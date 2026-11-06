# The web build

**Status:** revision 1 · 2026-11-06 · the window built for a browser (M6.4)

OpenMailClient's window runs in a web browser: the same engine, the same egui window, built
for WebAssembly (`wasm32-unknown-unknown`) and drawn with WebGL on one canvas. It is the base
that web access (M7) is built on; on its own it reaches nothing beyond the page.

## Building and trying it

```bash
cargo install wasm-bindgen-cli --version 0.2.129
cargo xtask web
python -m http.server 8766 --bind 127.0.0.1 --directory target/web
```

The CLI's version must be the one `Cargo.lock` names for `wasm-bindgen`. `cargo xtask web`
builds `apps/openmailclient-web` for release, runs `wasm-bindgen --target web` and writes
`index.html` and `start.js` beside the module in `target/web`. Any static server will do;
open `http://127.0.0.1:8766/`.

## What is the same and what is not

- The sample mailbox opens, and Mail, Calendar, People and Tasks work on it as on the
  desktop: reading, writing, flags, categories, appointments, the To-Do Bar.
- The clock and the time zone are the browser's (`web-time`, chrono's `wasmbind`).
- There are no threads: work that runs on a thread of its own on the desktop (saves,
  exports, pictures decoded) runs in place, so a large one holds the page while it runs.
- There are no files, sockets or credential store in a page. Nothing is sent, received or
  saved; settings are not kept between visits. Opening a data file from the browser and
  keeping settings in its storage are M6.6; SVG drawings, which need a larger stack than a
  page has, too.
- The page loads nothing from anywhere but the folder it is served from. Its Content
  Security Policy allows scripts only from there and WebAssembly to be compiled; no script
  is inline. Message content is drawn by our own layout, as on the desktop: no HTML from a
  message reaches the browser's own engine.

## Revision history

- 1 · 2026-11-06 — first version: `cargo xtask web`, the sample mailbox in a browser.
