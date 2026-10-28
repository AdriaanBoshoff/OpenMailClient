//! Updates of the program, from its releases on GitHub: what the newest release is, read from
//! GitHub's list of them ([GitHub REST API, Releases]); its installer brought down and checked
//! against the SHA-256 sums published beside it; and the installer run once the program has
//! closed, with every unsaved change saved or journalled first.
//!
//! Nothing here is done on its own but asking what the newest release is, once a day, and that
//! only while `Updates::automatic` is on. What GitHub says is hostile like any server's: the list
//! is capped, each release's words are capped and shown as text, and a file is only ever taken
//! from this project's own release addresses on github.com. Pre-releases are offered when asked
//! for, and always to a pre-release: the newest of either kind that is newer than this one.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The project whose releases are this program's.
pub const REPOSITORY: &str = "AdriaanBoshoff/OpenMailClient";
/// This program's version, as its releases are tagged without the `v`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// GitHub's list of the project's releases, the newest first.
pub const RELEASES: &str = "https://api.github.com/repos/AdriaanBoshoff/OpenMailClient/releases?per_page=30";
/// Where a release's files are handed out from, and where its page is.
const DOWNLOADS: &str = "https://github.com/AdriaanBoshoff/OpenMailClient/releases/download/";
const PAGES: &str = "https://github.com/AdriaanBoshoff/OpenMailClient/releases/";
/// The name of the file of sums published with each release.
pub const SUMS: &str = "SHA256SUMS.txt";

/// How the work is named while it runs.
pub const CHECKING: &str = "Checking for updates";
pub const DOWNLOADING: &str = "Downloading the update";

/// The longest list of releases read.
pub const MAX_LIST: usize = 4 << 20;
/// Releases read from it at most.
const MAX_RELEASES: usize = 100;
/// Characters of a release's notes kept.
const MAX_NOTES: usize = 20_000;
/// The longest file of sums read.
pub const MAX_SUMS: u64 = 64 << 10;
/// The largest installer brought down.
pub const MAX_SETUP: u64 = 400 << 20;
/// How often the newest release is asked after on its own, and asked again after a failure.
pub const CHECK_EVERY_SECS: i64 = 24 * 60 * 60;
pub const RETRY_SECS: i64 = 60 * 60;

/// A version as semantic versioning reads it ([SemVer 2.0.0] 2, 9 to 11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// What follows the `-`: `alpha.1` is a pre-release coming before `0.1.0` itself.
    pub pre: Vec<Part>,
}

/// One dot-separated part of a pre-release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Number(u64),
    Word(String),
}

impl Version {
    /// Read `1.2.3`, `v1.2.3-alpha.1` or `1.2.3+build`; anything else is not a version.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        if text.is_empty() || text.len() > 100 {
            return None;
        }
        // What follows a `+` is build metadata, which plays no part in which is newer (10).
        let text = text.split('+').next().unwrap_or("");
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.').map(number);
        let (major, minor, patch) = (numbers.next()??, numbers.next()??, numbers.next()??);
        if numbers.next().is_some() {
            return None;
        }
        let mut parts = Vec::new();
        if let Some(pre) = pre {
            for part in pre.split('.') {
                if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return None;
                }
                parts.push(match number(part) {
                    Some(n) => Part::Number(n),
                    None if part.chars().all(|c| c.is_ascii_digit()) => return None,
                    None => Part::Word(part.to_string()),
                });
            }
        }
        Some(Version { major, minor, patch, pre: parts })
    }

    pub fn is_pre(&self) -> bool {
        !self.pre.is_empty()
    }
}

/// A number without leading zeros (2).
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit()) || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    text.parse().ok()
}

impl Ord for Part {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Part::Number(a), Part::Number(b)) => a.cmp(b),
            (Part::Word(a), Part::Word(b)) => a.cmp(b),
            // Numbers come before words (11.4.3).
            (Part::Number(_), Part::Word(_)) => std::cmp::Ordering::Less,
            (Part::Word(_), Part::Number(_)) => std::cmp::Ordering::Greater,
        }
    }
}

impl PartialOrd for Part {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            // A pre-release comes before the release itself (11.3); otherwise part by part,
            // the one with more parts after when all before are the same (11.4.4).
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (i, part) in self.pre.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match part {
                Part::Number(n) => write!(f, "{n}")?,
                Part::Word(w) => f.write_str(w)?,
            }
        }
        Ok(())
    }
}

/// This program's own version.
pub fn current() -> Version {
    Version::parse(VERSION).unwrap_or(Version { major: 0, minor: 0, patch: 0, pre: Vec::new() })
}

/// A file of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    /// Where it is handed out: always under this project's own release addresses.
    pub url: String,
    pub size: u64,
}

/// A release as GitHub lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    /// Its title, and what it says of itself: written by the project, shown only as text.
    pub name: String,
    pub notes: String,
    pub prerelease: bool,
    /// Its page on GitHub.
    pub page: String,
    /// When it was published, as GitHub says it (RFC 3339).
    pub published: String,
    /// The installer for Windows, and the sums of its files. It is run only on Windows
    /// (`cmd::update::install`); other systems install a release from its page.
    pub setup: Option<Asset>,
    pub sums: Option<Asset>,
}

/// The installer's name in a release of `version`.
pub fn setup_name(version: &Version) -> String {
    format!("OpenMailClient-{version}-windows-x64-setup.exe")
}

/// Text from GitHub fit to keep: no control characters but line ends, and at most `max` of them.
fn text(v: Option<&Value>, max: usize) -> String {
    v.and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect()
}

/// The releases in GitHub's list of them, drafts and those not tagged with a version left out.
pub fn read_releases(bytes: &[u8]) -> Result<Vec<Release>, String> {
    if bytes.len() > MAX_LIST {
        return Err("the list of releases is too long".into());
    }
    let Ok(Value::Array(all)) = serde_json::from_slice::<Value>(bytes) else {
        return Err("what GitHub sent is not a list of releases".into());
    };
    let mut out = Vec::new();
    for r in all.iter().take(MAX_RELEASES) {
        if r.get("draft").and_then(Value::as_bool) != Some(false) {
            continue;
        }
        let tag = text(r.get("tag_name"), 100);
        let Some(version) = Version::parse(&tag) else { continue };
        let page = text(r.get("html_url"), 500);
        let page = if page.starts_with(PAGES) && !page.contains("..") { page } else { format!("{PAGES}tag/{tag}") };
        let assets: Vec<Asset> = r
            .get("assets")
            .and_then(Value::as_array)
            .map(|assets| {
                assets
                    .iter()
                    .take(100)
                    .filter_map(|a| {
                        let name = text(a.get("name"), 200);
                        let url = text(a.get("browser_download_url"), 1000);
                        // Only this project's own files, and only by their own names.
                        let own = url.strip_prefix(DOWNLOADS).is_some_and(|rest| rest.split('/').count() == 2 && rest.ends_with(&format!("/{name}")));
                        let size = a.get("size").and_then(Value::as_u64).unwrap_or(0);
                        (own && !name.is_empty() && !name.contains("..")).then_some(Asset { name, url, size })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let wanted = setup_name(&version);
        out.push(Release {
            tag: tag.clone(),
            name: text(r.get("name"), 200),
            notes: text(r.get("body"), MAX_NOTES),
            prerelease: r.get("prerelease").and_then(Value::as_bool).unwrap_or(false) || version.is_pre(),
            page,
            published: text(r.get("published_at"), 40),
            setup: assets.iter().find(|a| a.name == wanted).cloned(),
            sums: assets.iter().find(|a| a.name == SUMS).cloned(),
            version,
        });
    }
    Ok(out)
}

/// The newest release newer than `than`: a pre-release only when they are asked for or `than`
/// is one itself.
pub fn newest<'a>(releases: &'a [Release], than: &Version, prereleases: bool) -> Option<&'a Release> {
    let pre = prereleases || than.is_pre();
    releases.iter().filter(|r| r.version > *than && (pre || !r.prerelease)).max_by(|a, b| a.version.cmp(&b.version))
}

/// The SHA-256 of `name` in a file of sums as `sha256sum` writes them: the sum in hexadecimal,
/// a space, a space or `*`, and the name.
pub fn sum_for(sums: &str, name: &str) -> Option<[u8; 32]> {
    for line in sums.lines().take(1000) {
        let line = line.trim_end();
        let Some((sum, rest)) = line.split_once(' ') else { continue };
        let rest = rest.strip_prefix([' ', '*']).unwrap_or(rest);
        if rest == name {
            return unhex(sum);
        }
    }
    None
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks(2).enumerate() {
        let digits = std::str::from_utf8(pair).ok()?;
        *out.get_mut(i)? = u8::from_str_radix(digits, 16).ok()?;
    }
    Some(out)
}

pub fn hex(sum: &[u8; 32]) -> String {
    sum.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of a file, read a piece at a time.
pub fn sum_of_file(path: &Path) -> std::io::Result<[u8; 32]> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 << 10];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
    Ok(hasher.finalize().into())
}

/// Bytes written to a file and summed as they go.
pub(crate) struct Summed {
    pub file: std::fs::File,
    pub hasher: Sha256,
}

impl std::io::Write for Summed {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hasher.update(buf.get(..n).unwrap_or_default());
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// An installer brought down and checked, waiting to be run.
#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub path: PathBuf,
    pub sha256: [u8; 32],
    pub version: Version,
}

/// Where updates stand this run.
#[derive(Clone, Debug, Default)]
pub struct Updates {
    /// Whether pre-releases are offered (`help.updateOptions`). A pre-release is offered newer
    /// pre-releases whatever this says.
    pub prereleases: bool,
    /// Whether the newest release is asked after on its own once a day. It only ever tells.
    pub automatic: bool,
    /// When it was last asked after with an answer, in Unix seconds; and last tried this run.
    pub last_checked: i64,
    pub last_tried: Option<i64>,
    /// Whether a check has come back this run, and the newest newer release it found.
    pub checked: bool,
    pub found: Option<Release>,
    /// What the last check or download said when it failed.
    pub failed: Option<String>,
    /// The installer brought down and checked.
    pub ready: Option<Ready>,
    /// Asked to be run once the program has closed.
    pub install: Option<Ready>,
}

impl Updates {
    pub fn new() -> Self {
        Updates { prereleases: current().is_pre(), automatic: true, ..Updates::default() }
    }
}

/// Whether `path` is an installer of ours in the folder of updates.
pub fn is_ours(path: &Path, dir: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    path.parent() == Some(dir) && name.starts_with("OpenMailClient-") && name.ends_with("-setup.exe") && !name.contains("..")
}

/// Run an installer that was brought down, once more checked to be the one that was, quietly:
/// it shows how far it is, keeps the choices of the install before it (the Start menu and the
/// desktop), and starts the program again when it is done. Only on Windows.
pub fn launch(ready: &Ready, dir: &Path) -> Result<(), String> {
    if !is_ours(&ready.path, dir) {
        return Err("that is not an update this program brought down".into());
    }
    let sum = sum_of_file(&ready.path).map_err(|e| format!("the update could not be read: {e}"))?;
    if sum != ready.sha256 {
        return Err("the update has changed since it was checked, and is not run".into());
    }
    if !cfg!(windows) {
        return Err("updates are installed this way only on Windows".into());
    }
    std::process::Command::new(&ready.path)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELAUNCH=1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("the installer could not be started: {e}"))
}
