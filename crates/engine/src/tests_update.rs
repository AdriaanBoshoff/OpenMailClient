//! Updates from the program's releases, against the fake web server: nothing leaves the
//! process, and nothing is run.

use crate::Session;
use crate::update::{self, Version, newest, read_releases, sum_for};
use openmailclient_net::fake::{Fake, State};
use serde_json::{Value, json};
use sha2::Digest;
use std::sync::Arc;

const API: &str = "api.github.com/repos/AdriaanBoshoff/OpenMailClient/releases?per_page=30";
const FILES: &str = "https://github.com/AdriaanBoshoff/OpenMailClient/releases/download/";

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn versions_are_ordered_as_semantic_versioning_orders_them() {
    // The order SemVer 2.0.0 11 gives.
    let ordered = [
        "0.9.9",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.1.0-alpha.1",
        "1.10.0",
        "2.0.0",
    ];
    for pair in ordered.windows(2) {
        assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
    }
    assert_eq!(v("v1.2.3+build.5"), v("1.2.3"));
    assert_eq!(v("0.1.0-alpha.1").to_string(), "0.1.0-alpha.1");
    assert!(v("0.1.0-alpha.1").is_pre() && !v("0.1.0").is_pre());
    for junk in
        ["", "1", "1.2", "1.2.3.4", "01.2.3", "1.2.3-", "1.2.3-a..b", "1.2.3-01", "1.2.3-é", "x.y.z", "99999999999999999999.0.0", &"1".repeat(200)]
    {
        assert!(Version::parse(junk).is_none(), "{junk}");
    }
    assert!(update::current().to_string() == update::VERSION, "the program's own version is read");
}

/// A release as GitHub lists it, with an installer and its sums.
fn release(tag: &str, pre: bool) -> Value {
    let version = tag.trim_start_matches('v');
    json!({
        "tag_name": tag,
        "name": format!("OpenMailClient {version}"),
        "body": "What is new",
        "draft": false,
        "prerelease": pre,
        "html_url": format!("https://github.com/AdriaanBoshoff/OpenMailClient/releases/tag/{tag}"),
        "published_at": "2026-10-25T10:00:00Z",
        "assets": [
            { "name": format!("OpenMailClient-{version}-windows-x64-setup.exe"), "size": 3000, "browser_download_url": format!("{FILES}{tag}/OpenMailClient-{version}-windows-x64-setup.exe") },
            { "name": "SHA256SUMS.txt", "size": 100, "browser_download_url": format!("{FILES}{tag}/SHA256SUMS.txt") },
        ],
    })
}

#[test]
fn releases_are_read_warily_and_the_newest_wanted_is_chosen() {
    let mut hostile = release("v9.0.0", false);
    hostile["assets"][0]["browser_download_url"] = json!("https://evil.example.com/OpenMailClient-9.0.0-windows-x64-setup.exe");
    hostile["html_url"] = json!("https://evil.example.com/");
    hostile["body"] = json!("\u{202e}x\u{0}".repeat(50_000));
    let mut draft = release("v10.0.0", false);
    draft["draft"] = json!(true);
    let list = json!([
        release("v0.0.1", false),
        release("v2.0.0-beta.1", true),
        release("v1.0.0", false),
        hostile,
        draft,
        { "tag_name": "nightly", "draft": false },
        { "tag_name": 7 },
        "not a release",
    ]);
    let all = read_releases(list.to_string().as_bytes()).unwrap();
    assert_eq!(all.len(), 4, "drafts and tags that are not versions are left out");
    let nine = all.iter().find(|r| r.tag == "v9.0.0").unwrap();
    assert!(nine.setup.is_none(), "a file from anywhere but this project's releases is not taken");
    assert!(nine.page.starts_with("https://github.com/AdriaanBoshoff/OpenMailClient/releases/"));
    assert!(nine.notes.chars().count() <= 20_000 && !nine.notes.contains('\u{0}'));
    // Stable only, from a stable release; pre-releases when asked, or from a pre-release.
    assert_eq!(newest(&all, &v("0.5.0"), false).map(|r| r.tag.as_str()), Some("v9.0.0"));
    assert_eq!(newest(&all, &v("9.0.0"), false), None);
    assert_eq!(newest(&all[..3], &v("1.0.0"), true).map(|r| r.tag.as_str()), Some("v2.0.0-beta.1"));
    assert_eq!(newest(&all[..3], &v("1.0.0"), false), None);
    assert_eq!(newest(&all[..3], &v("1.0.1-alpha.1"), false).map(|r| r.tag.as_str()), Some("v2.0.0-beta.1"));
    let ours = &all[2];
    assert_eq!(ours.setup.as_ref().map(|a| a.size), Some(3000));
    assert!(ours.sums.is_some());
    for junk in [&b""[..], b"{}", b"[", b"\xff\xfe", b"null"] {
        assert!(read_releases(junk).is_err());
    }
    assert!(read_releases(&vec![b' '; update::MAX_LIST + 1]).is_err());
}

#[test]
fn sums_are_read_as_sha256sum_writes_them() {
    let sum = "ab".repeat(32);
    let text = format!("{}  other.exe\n{sum} *OpenMailClient-1.0.0-windows-x64-setup.exe\r\nzz  broken\n", "cd".repeat(32));
    assert_eq!(sum_for(&text, "OpenMailClient-1.0.0-windows-x64-setup.exe"), Some([0xab; 32]));
    assert_eq!(sum_for(&text, "other.exe"), Some([0xcd; 32]));
    assert_eq!(sum_for(&text, "broken"), None);
    assert_eq!(sum_for(&text, "missing.exe"), None);
    assert_eq!(sum_for(&format!("{}  x", "g".repeat(64)), "x"), None);
}

fn answer(body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

/// A session that asks the fake GitHub, which has a release of `tag` with `setup` as its
/// installer and `sums` as its sums; and a folder for updates of its own.
fn asking(tag: &str, setup: &[u8], sums: &str, name: &str) -> (Session, Fake, std::path::PathBuf) {
    let version = tag.trim_start_matches('v');
    let mut r = release(tag, false);
    r["assets"][0]["size"] = json!(setup.len());
    let installer = format!("OpenMailClient-{version}-windows-x64-setup.exe");
    let signed = format!("/f/{installer}?sig={}", "s".repeat(1500));
    let pages = vec![
        (API.to_string(), answer(json!([r]).to_string().as_bytes())),
        (
            format!("github.com/AdriaanBoshoff/OpenMailClient/releases/download/{tag}/{installer}"),
            format!("HTTP/1.1 302 Found\r\nLocation: https://objects.example.com{signed}\r\n\r\n").into_bytes(),
        ),
        (format!("objects.example.com{signed}"), answer(setup)),
        (format!("github.com/AdriaanBoshoff/OpenMailClient/releases/download/{tag}/SHA256SUMS.txt"), answer(sums.as_bytes())),
    ];
    let fake = Fake::new(State { pages, ..State::default() });
    let mut s = Session::new();
    s.connector = Some(Arc::new(fake.clone()));
    let dir = std::env::temp_dir().join(format!("omc-updates-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.update_dir = Some(dir.clone());
    (s, fake, dir)
}

#[test]
fn an_update_is_found_brought_down_checked_and_kept_for_the_close() {
    let setup: Vec<u8> = (0..300_000u32).map(|n| (n % 253) as u8).collect();
    let sum = sha2::Sha256::digest(&setup);
    let name = "OpenMailClient-99.0.0-windows-x64-setup.exe";
    let sums = format!("{}  {name}\n", sum.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let (mut s, fake, dir) = asking("v99.0.0", &setup, &sums, "good");
    // Nothing is brought down before a release is found, and nothing installed before it is down.
    assert!(s.run("help.downloadUpdate", &json!({ "wait": true })).is_err());
    assert!(s.run("help.installUpdate", &Value::Null).is_err());
    let found = s.run("help.checkForUpdates", &json!({ "wait": true })).unwrap();
    assert_eq!(found["newer"], true);
    assert_eq!(found["untrusted_content"]["release"]["version"], "99.0.0");
    assert!(s.status.contains("99.0.0 is available"), "{}", s.status);
    let heard = fake.state().heard.join("\n");
    assert!(heard.contains("GET /repos/AdriaanBoshoff/OpenMailClient/releases?per_page=30"), "{heard}");
    // Asked on its own a moment later: not again for a day.
    assert_eq!(s.run("help.checkForUpdates", &json!({ "auto": true, "wait": true })).unwrap()["started"], false);
    let got = s.run("help.downloadUpdate", &json!({ "wait": true })).unwrap();
    assert_eq!(got["ready"], true, "{got}");
    let ready = s.update.ready.clone().unwrap();
    assert_eq!(std::fs::read(&ready.path).unwrap(), setup);
    assert!(update::is_ours(&ready.path, &dir));
    assert!(!dir.join(format!("{name}.part")).exists());
    if cfg!(windows) {
        let asked = s.run("help.installUpdate", &Value::Null).unwrap();
        assert_eq!(asked["installOnClose"], true);
        assert!(s.update.install.is_some());
        s.run("help.installUpdate", &json!({ "cancel": true })).unwrap();
        assert!(s.update.install.is_none());
        // Changed after it was checked: neither kept for the close nor run.
        std::fs::write(&ready.path, b"changed").unwrap();
        assert!(s.run("help.installUpdate", &Value::Null).is_err());
        assert!(update::launch(&ready, &dir).is_err());
    }
    // Only an installer of ours in the folder of updates is ever run.
    let mut elsewhere = ready.clone();
    elsewhere.path = std::env::temp_dir().join("OpenMailClient-1.0.0-windows-x64-setup.exe");
    assert!(update::launch(&elsewhere, &dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_installer_that_is_not_the_one_its_sums_name_is_thrown_away() {
    let setup = b"an installer".to_vec();
    let name = "OpenMailClient-99.0.0-windows-x64-setup.exe";
    for (case, sums) in
        [("wrong", format!("{}  {name}\n", "00".repeat(32))), ("unnamed", format!("{}  other.exe\n", "00".repeat(32))), ("junk", "\u{0}".repeat(500))]
    {
        let (mut s, _fake, dir) = asking("v99.0.0", &setup, &sums, case);
        s.run("help.checkForUpdates", &json!({ "wait": true })).unwrap();
        let got = s.run("help.downloadUpdate", &json!({ "wait": true })).unwrap();
        assert_eq!(got["ok"], false, "{case}: {got}");
        assert!(s.update.ready.is_none(), "{case}");
        assert!(s.update.failed.is_some(), "{case}");
        let left: Vec<_> = std::fs::read_dir(&dir).map(|d| d.flatten().collect()).unwrap_or_default();
        assert!(left.is_empty(), "{case}: nothing is left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn no_newer_release_or_no_server_says_so_and_options_are_kept() {
    let (mut s, _fake, dir) = asking("v0.0.1", b"old", "", "older");
    let found = s.run("help.checkForUpdates", &json!({ "wait": true })).unwrap();
    assert_eq!(found["newer"], false);
    assert!(s.status.contains("is the newest"), "{}", s.status);
    assert!(s.update.last_checked > 0);
    // A server that is not GitHub's answer: said, and asked again in an hour, not at once.
    let mut s = Session::new();
    s.connector = Some(Arc::new(Fake::new(State::default())));
    s.status = "Ready".into();
    let checked = s.run("help.checkForUpdates", &json!({ "auto": true, "wait": true })).unwrap();
    assert_eq!(checked["ok"], false);
    assert_eq!(s.status, "Ready", "a check on its own leaves the status line be");
    assert!(!s.update_due());
    // The choices are kept in the settings, a time in the future left out.
    s.run("help.updateOptions", &json!({ "prereleases": true, "automatic": false })).unwrap();
    assert!(s.run("help.updateOptions", &json!({ "prereleases": "yes" })).is_err());
    s.update.last_checked = 1_700_000_000;
    let file = std::env::temp_dir().join(format!("omc-update-settings-{}.json", std::process::id()));
    s.run("settings.save", &json!({ "path": file.display().to_string() })).unwrap();
    let mut next = Session::new();
    next.run("settings.load", &json!({ "path": file.display().to_string() })).unwrap();
    assert!(next.update.prereleases && !next.update.automatic);
    assert_eq!(next.update.last_checked, 1_700_000_000);
    std::fs::write(&file, r#"{"updates": {"lastChecked": 99999999999999, "automatic": 3}}"#).unwrap();
    let mut odd = Session::new();
    let took = odd.run("settings.load", &json!({ "path": file.display().to_string() })).unwrap();
    assert_eq!(took["ignored"], 2);
    assert_eq!(odd.update.last_checked, 0);
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&dir);
}
