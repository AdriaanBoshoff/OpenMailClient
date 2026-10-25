//! `cargo xtask package`: the program and the CLI built for release, the Windows installer made
//! from `installer/openmailclient.iss` with Inno Setup, and `SHA256SUMS.txt` beside it, all in
//! `target/dist`. Check for Updates finds the installer by its name and checks it by those sums
//! (`crates/engine/src/update.rs`), so the names here and there are the same.

use super::{cargo, root};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn package() -> Result<(), String> {
    if !cfg!(windows) {
        return Err("the installer is made on Windows; other systems are not packaged yet".into());
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
