//! `cargo xtask web`: OpenMailClient built for the web (M6), into `target/web`.
//!
//! `apps/openmailclient-web` is built for wasm32. `wasm-bindgen` (the CLI of the same version
//! as the library in `Cargo.lock`, `cargo install wasm-bindgen-cli --version <it>`) writes the
//! module and the loader it makes for it. The page is written here, around one canvas. Nothing
//! on the page is fetched from anywhere but the folder it is served from.

use crate::{cargo, root};
use std::process::Command;

/// The page: one canvas the window is drawn on, the whole of the browser's window, and the
/// module `wasm-bindgen` wrote, which starts the program.
fn page(version: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'">
<title>OpenMailClient</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; overflow: hidden; background: #f3f3f3; }}
  @media (prefers-color-scheme: dark) {{ html, body {{ background: #1f1f1f; }} }}
  canvas {{ display: block; width: 100%; height: 100%; }}
</style>
</head>
<body>
<canvas id="openmailclient"></canvas>
<noscript>OpenMailClient {version} needs WebAssembly.</noscript>
<script type="module" src="start.js"></script>
</body>
</html>
"#
    )
}

/// The one line that loads the module: it is written by this task, not by hand, so that the
/// page needs no script of its own and the policy above allows none inline.
const START: &str = "import init from './openmailclient.js';\ninit();\n";

pub fn web() -> Result<(), String> {
    cargo(&["build", "--release", "--target", "wasm32-unknown-unknown", "-p", "openmailclient-web"])?;
    let root = root();
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), std::path::PathBuf::from);
    let wasm = target.join("wasm32-unknown-unknown").join("release").join("openmailclient-web.wasm");
    let out = root.join("target").join("web");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("> wasm-bindgen {}", wasm.display());
    let status = Command::new("wasm-bindgen")
        .arg(&wasm)
        .args(["--target", "web", "--no-typescript", "--out-name", "openmailclient", "--out-dir"])
        .arg(&out)
        .status()
        .map_err(|e| format!("wasm-bindgen: {e}; install it with `cargo install wasm-bindgen-cli --version` and the version Cargo.lock names"))?;
    if !status.success() {
        return Err("wasm-bindgen failed".to_string());
    }
    let version = env!("CARGO_PKG_VERSION");
    for (name, text) in [("index.html", page(version)), ("start.js", START.to_string())] {
        let path = out.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let size = std::fs::metadata(out.join("openmailclient_bg.wasm")).map(|m| m.len()).unwrap_or(0);
    println!("web: {} ({:.1} MB of WebAssembly); serve the folder and open index.html", out.display(), size as f64 / 1e6);
    Ok(())
}
