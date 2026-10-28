//! `cargo xtask package`: the program and the CLI built for release, the Windows installer made
//! from `installer/openmailclient.iss` with Inno Setup, and `SHA256SUMS.txt` beside it, all in
//! `target/dist`. Check for Updates finds the installer by its name and checks it by those sums
//! (`crates/engine/src/update.rs`), so the names here and there are the same.
//!
//! On Linux it makes `OpenMailClient-<version>-linux-x64.tar.gz` instead: the two programs, the
//! icon drawn by `app_icon`, a desktop entry and `install.sh`, which installs them for the one
//! who runs it, under `~/.local`, and takes them away again with `--uninstall`. With
//! `cargo-zigbuild` and `zig` installed the programs are linked against glibc 2.31, so the one
//! archive runs on Ubuntu 20.04, Debian 11 and every distribution since.

use super::{app_icon, cargo, root};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn package() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        return linux();
    }
    if !cfg!(windows) {
        return Err("packages are made on Windows and Linux; other systems are not packaged yet".into());
    }
    let version = env!("CARGO_PKG_VERSION");
    cargo(&["build", "--release", "-p", "openmailclient", "-p", "openmailclient-cli"])?;
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from);
    let built = target.join("release");
    let iscc = iscc().ok_or("Inno Setup 6 was not found: winget install --id JRSoftware.InnoSetup -e, or set ISCC to ISCC.exe")?;
    let script = root().join("installer").join("openmailclient.iss");
    println!("> {} {}", iscc.display(), script.display());
    let status = Command::new(&iscc)
        .arg(format!("/DAppVersion={version}"))
        .arg(format!("/DNumericVersion={}", numeric(version)))
        .arg(format!("/DSourceDir={}", built.display()))
        .arg(format!("/O{}", target.join("dist").display()))
        .arg("/Q")
        .arg(&script)
        .status()
        .map_err(|e| format!("cannot run Inno Setup: {e}"))?;
    if !status.success() {
        return Err("Inno Setup could not make the installer".into());
    }
    let dist = target.join("dist");
    let setup = format!("OpenMailClient-{version}-windows-x64-setup.exe");
    let sum = sha256(&dist.join(&setup))?;
    // The form `sha256sum` writes and checks: the sum, two spaces, the name.
    std::fs::write(dist.join("SHA256SUMS.txt"), format!("{sum}  {setup}\n")).map_err(|e| e.to_string())?;
    println!("package: {}", dist.join(&setup).display());
    println!("package: {}  {setup}", sum);
    Ok(())
}

/// The desktop entry ([freedesktop Desktop Entry Specification] 1.5); `Exec` is set by
/// `install.sh` to where the program is put.
const DESKTOP: &str = "[Desktop Entry]
Type=Application
Version=1.5
Name=OpenMailClient
GenericName=Mail Client
Comment=Mail, calendar, contacts and tasks
Exec=openmailclient %F
Icon=openmailclient
Terminal=false
Categories=Office;Network;Email;Calendar;ContactManagement;
MimeType=message/rfc822;text/calendar;text/vcard;text/x-vcard;
StartupWMClass=openmailclient
";

/// Installs the programs for the one who runs it, under `~/.local` (no administrator), or takes
/// them away with `--uninstall`; mail, settings and accounts are left where they are.
const INSTALL: &str = r#"#!/bin/sh
# OpenMailClient for Linux: installs for you alone, under ~/.local; --uninstall takes it away.
# Your mail, settings and accounts are kept either way.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin=$HOME/.local/bin
lib=$data/openmailclient
if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin/openmailclient" "$bin/openmailclient-cli" "$data/applications/openmailclient.desktop" "$data/icons/hicolor/256x256/apps/openmailclient.png"
    rm -rf "$lib"
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
    echo "OpenMailClient is uninstalled; your mail and settings are where they were."
    exit 0
fi
mkdir -p "$bin" "$lib" "$data/applications" "$data/icons/hicolor/256x256/apps"
cp "$here/openmailclient" "$here/openmailclient-cli" "$lib/"
chmod 755 "$lib/openmailclient" "$lib/openmailclient-cli"
ln -sf "$lib/openmailclient" "$bin/openmailclient"
ln -sf "$lib/openmailclient-cli" "$bin/openmailclient-cli"
cp "$here/openmailclient.png" "$data/icons/hicolor/256x256/apps/openmailclient.png"
sed "s|^Exec=.*|Exec=$lib/openmailclient %F|" "$here/openmailclient.desktop" > "$data/applications/openmailclient.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
echo "OpenMailClient is installed: find it among your applications, or run $bin/openmailclient"
case ":$PATH:" in *":$bin:"*) ;; *) echo "($bin is not on your PATH; add it to run openmailclient-cli by name.)" ;; esac
"#;

/// The Linux archive: the programs, the icon, the desktop entry and `install.sh`, in a folder
/// named for the version, packed with `tar`.
fn linux() -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from);
    // A program links against the newest symbol versions of the glibc it is built on, and does
    // not start on a system with an older one. Built by `cargo zigbuild` against GLIBC (2.31,
    // Ubuntu 20.04 and Debian 11, unless set), it starts on those and every one since.
    let built = if zigbuild() {
        let glibc = std::env::var("GLIBC").unwrap_or_else(|_| "2.31".into());
        let triple = format!("x86_64-unknown-linux-gnu.{glibc}");
        cargo(&["zigbuild", "--release", "--target", &triple, "-p", "openmailclient", "-p", "openmailclient-cli"])?;
        target.join("x86_64-unknown-linux-gnu").join("release")
    } else {
        println!("package: cargo-zigbuild and zig were not found, so the programs need this system's glibc or a newer one");
        cargo(&["build", "--release", "-p", "openmailclient", "-p", "openmailclient-cli"])?;
        target.join("release")
    };
    let dist = target.join("dist");
    let name = format!("OpenMailClient-{version}-linux-x64");
    let folder = dist.join(&name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let copy = |from: &Path, to: &str| std::fs::copy(from, folder.join(to)).map(|_| ()).map_err(|e| format!("{}: {e}", from.display()));
    copy(&built.join("openmailclient"), "openmailclient")?;
    copy(&built.join("openmailclient-cli"), "openmailclient-cli")?;
    for doc in ["README.md", "LICENSE-MIT", "LICENSE-APACHE", "ATTRIBUTION.md"] {
        copy(&root().join(doc), doc)?;
    }
    let png = image::RgbaImage::from_raw(256, 256, app_icon::rgba(256)).ok_or("the icon came out the wrong size")?;
    png.save(folder.join("openmailclient.png")).map_err(|e| e.to_string())?;
    let write = |to: &str, text: &str| std::fs::write(folder.join(to), text).map_err(|e| format!("{to}: {e}"));
    write("openmailclient.desktop", DESKTOP)?;
    write("install.sh", INSTALL)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for run in ["openmailclient", "openmailclient-cli", "install.sh"] {
            std::fs::set_permissions(folder.join(run), std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{run}: {e}"))?;
        }
    }
    let archive = format!("{name}.tar.gz");
    println!("> tar -czf {archive} {name}");
    let status = Command::new("tar")
        .arg("--owner=0")
        .arg("--group=0")
        .arg("--mode=u+rwX,go+rX,go-w")
        .arg("-czf")
        .arg(dist.join(&archive))
        .arg("-C")
        .arg(&dist)
        .arg(&name)
        .status()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    if !status.success() {
        return Err("tar could not make the archive".into());
    }
    let sum = sha256(&dist.join(&archive))?;
    std::fs::write(dist.join("SHA256SUMS.txt"), format!("{sum}  {archive}\n")).map_err(|e| e.to_string())?;
    println!("package: {}", dist.join(&archive).display());
    println!("package: {sum}  {archive}");
    Ok(())
}

/// Whether `cargo zigbuild` and `zig` both run.
fn zigbuild() -> bool {
    let program = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let runs = |c: &mut Command| c.output().is_ok_and(|o| o.status.success());
    runs(Command::new(program).args(["zigbuild", "--help"])) && runs(Command::new("zig").arg("version"))
}

/// Where Inno Setup's compiler is: `ISCC`, a per-user install, an install for everyone, `PATH`.
fn iscc() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ISCC").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(path);
    }
    let mut places = Vec::new();
    for var in ["LOCALAPPDATA", "ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(dir) = std::env::var_os(var) {
            let dir = PathBuf::from(dir);
            places.push(dir.join("Programs").join("Inno Setup 6").join("ISCC.exe"));
            places.push(dir.join("Inno Setup 6").join("ISCC.exe"));
        }
    }
    places.into_iter().find(|p| p.is_file()).or_else(|| Command::new("ISCC.exe").arg("/?").output().ok().map(|_| PathBuf::from("ISCC.exe")))
}

/// The four numbers Windows keeps of a version: `0.1.0-alpha.1` is 0.1.0.0.
fn numeric(version: &str) -> String {
    let core = version.split(['-', '+']).next().unwrap_or("0");
    let mut parts: Vec<&str> = core.split('.').take(3).collect();
    parts.resize(3, "0");
    format!("{}.0", parts.join("."))
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
