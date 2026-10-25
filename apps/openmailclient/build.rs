//! On Windows the program file carries its icon and version, made from `assets/openmailclient.ico`
//! (`cargo xtask icon`) with the resource compiler of the toolchain: `windres` for GNU, `rc` for
//! MSVC. Without one the program builds all the same, with the window's own icon only.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/openmailclient.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else { return };
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let number = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
    let (major, minor, patch) = (number("CARGO_PKG_VERSION_MAJOR"), number("CARGO_PKG_VERSION_MINOR"), number("CARGO_PKG_VERSION_PATCH"));
    let icon = std::env::current_dir().map(|d| d.join("assets").join("openmailclient.ico")).unwrap_or_default();
    if !icon.is_file() {
        println!("cargo:warning=no assets/openmailclient.ico; run cargo xtask icon");
        return;
    }
    let rc = out.join("openmailclient.rc");
    let text = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "OpenMailClient contributors"
      VALUE "FileDescription", "OpenMailClient"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "OpenMailClient"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "openmailclient.exe"
      VALUE "LegalCopyright", "MIT OR Apache-2.0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    if std::fs::write(&rc, text).is_err() {
        return;
    }
    let gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let (made, ok) = if gnu {
        let made = out.join("openmailclient-res.o");
        let ok = Command::new("windres").arg(&rc).arg("-O").arg("coff").arg("-o").arg(&made).status().is_ok_and(|s| s.success());
        (made, ok)
    } else {
        let made = out.join("openmailclient.res");
        let ok = Command::new("rc").arg("/nologo").arg("/fo").arg(&made).arg(&rc).status().is_ok_and(|s| s.success());
        (made, ok)
    };
    if ok {
        println!("cargo:rustc-link-arg-bins={}", made.display());
    } else {
        println!("cargo:warning=the resource compiler was not found; openmailclient.exe has no icon of its own");
    }
}
