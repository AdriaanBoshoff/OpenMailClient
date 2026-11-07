# The web build

**Status:** revision 2 · 2026-11-07 · data files, settings and SVG drawings in the browser (M6.6)

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
- **Files.** A page has no paths. File › Open & Export › Open Data File asks the browser for
  files (`.pst`, `.ost`, `.msg`, `.eml`, `.oft`, `.mht`), reads each one chosen into the
  page's memory and opens it as the desktop opens a file (`Session::open_bytes`): a data file
  takes its place in the folder pane beside the others, a message opens in its window. What
  the file is comes from its name, or from its first bytes when the name does not say. A
  file over 1 GiB is refused before it is read, as a page holds at most 4 GiB.
- **Saving is a download.** The browser's copy of a data file is never written back: changes
  are kept in the page while it is open. Export to a Data File writes the mailbox shown, with
  every change, as a new Unicode `.pst` and hands it to the browser as a download
  (`Session::store_bytes`); what is written is checked as a whole before it is given. Save As
  gives the message open as a `.msg` the same way (`Session::item_bytes`, which can also give
  `.eml` and `.oft`). The rest of what asks for a file on the desktop says that it is not done
  in a browser yet.
- **Settings** are kept in the browser's own storage for this page (`localStorage`, under
  `openmailclient-settings`), as the desktop keeps its settings file: as they change, at most
  once a second, and read again when the page is next opened (`settings.load` with
  `"text"`). Settings that make no sense are left out, as on the desktop. When the browser
  keeps nothing (a private window, storage turned off or full) the status bar says so once,
  and the page works as before without them.
- **SVG drawings** are drawn. On the desktop a drawing is drawn on a thread of its own with a
  large stack, and given up after three seconds; a page has neither, and a panic there cannot
  be caught. So a drawing is first looked over without being built (`svg::shallow`): one
  nested more than 48 deep, with more than 64 references to its own parts, or with a document
  type, is left a placeholder, and what is drawn is kept at most 1024 pixels on a side. The
  page's stack is made 16 MiB (`.cargo/config.toml`) to give the reader room for what passes.
  Compressed drawings (`.svgz`) are unpacked to at most 4 MiB, here and on the desktop, and
  drawings in UTF-16 are read too.
- **No threads.** Work that runs on a thread of its own on the desktop (saves, exports,
  pictures decoded, drawings) runs in place, so a large one holds the page while it runs. It
  is not given to a web worker: in web access (M7) the engine and the data files stay on the
  host, which has threads, and the browser only draws, so a worker would serve only this
  page, which has no account of its own. Until then a mailbox of many thousand messages is
  better opened in the desktop app.
- There are no sockets or credential store in a page: nothing is sent or received, and Send/
  Receive, accounts, printing and updates stay on the desktop. A link the person opens from
  the link dialog opens in a new tab that knows nothing of this one (`noopener`).
- The page loads nothing from anywhere but the folder it is served from. Its Content
  Security Policy allows scripts only from there and WebAssembly to be compiled; no script
  is inline. Message content is drawn by our own layout, as on the desktop: no HTML from a
  message reaches the browser's own engine.

## Revision history

- 2 · 2026-11-07 — M6.6: data files opened from the browser and given back as downloads,
  settings in the browser's storage, SVG drawings drawn there; compressed and UTF-16 drawings;
  why long work is not given to a web worker.
- 1 · 2026-11-06 — first version: `cargo xtask web`, the sample mailbox in a browser.
