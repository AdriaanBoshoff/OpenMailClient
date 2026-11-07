//! OpenMailClient in a web browser (M6): the window the desktop app shows, built for
//! WebAssembly and drawn on a canvas with WebGL. `cargo xtask web` builds it into `target/web`,
//! with the page that starts it.
//!
//! In the browser there are no paths, no threads and no sockets. A data file or message is
//! opened from a file the person chooses, read into memory (M6.6), and given back as a
//! download: Export writes the mailbox shown, with its changes, as a new `.pst`, and Save As
//! the message open. Settings are kept in the browser's own storage for this page. What would
//! run on a thread of its own runs where it is asked for, and nothing reaches beyond the page
//! but a link the person opened.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// The id of the canvas the page gives the window.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const CANVAS: &str = "openmailclient";

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use openmailclient_engine::Session;
    use openmailclient_ui_egui::{MailUi, UiRequest};
    use serde_json::{Value, json};
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::closure::Closure;

    /// Where the settings are kept in the browser's storage.
    const SETTINGS_KEY: &str = "openmailclient-settings";
    /// The settings are written at most this often; a change waiting asks for a frame then.
    const KEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
    /// The largest file opened: it is held in the page's memory whole, and a page has at most
    /// 4 GiB.
    const MAX_OPENED: f64 = 1024.0 * 1024.0 * 1024.0;
    /// How long a download's address is kept before it is let go, in milliseconds.
    const DOWNLOAD_KEPT_MS: i32 = 60_000;

    /// Files chosen and read, waiting to be opened: what reading them gave, by name.
    type Picked = Rc<RefCell<Vec<(String, Result<Vec<u8>, String>)>>>;

    struct App {
        ui: MailUi,
        /// When the page started, by the browser's clock: what is due is counted from it.
        started: web_time::Instant,
        picked: Picked,
        /// The settings as they were last kept, and when they were last written.
        kept: Value,
        written: Option<web_time::Instant>,
        /// Whether keeping the settings failed already; it is said once.
        keep_failed: bool,
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.ui.ui(ui);
            let ctx = ui.ctx().clone();
            let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
            self.ui.session.tick_clock();
            if let Some(left) = self.ui.session.reminders_due(now) {
                ctx.request_repaint_after(std::time::Duration::from_millis(left));
            }
            self.open_picked();
            for request in std::mem::take(&mut self.ui.requests) {
                self.answer(request, &ctx);
            }
            if self.keep_settings() {
                ctx.request_repaint_after(KEEP_EVERY);
            }
        }
    }

    impl App {
        /// What the window asked of the page.
        fn answer(&mut self, request: UiRequest, ctx: &egui::Context) {
            let said = match request {
                UiRequest::OpenFile => pick(&self.picked, ".pst,.ost,.msg,.eml,.oft,.mht,.mhtml", ctx).err(),
                UiRequest::Export => match self.ui.session.store_bytes() {
                    Ok((name, data)) => download(&name, &data, "application/vnd.ms-outlook").err(),
                    Err(e) => Some(format!("The data file could not be written: {e}")),
                },
                UiRequest::SaveAs => match self.ui.session.item_bytes("msg") {
                    Ok((name, data)) => download(&name, &data, "application/vnd.ms-outlook").err(),
                    Err(e) => Some(format!("The message could not be saved: {e}")),
                },
                // An address the engine checked, asked for by the person in the link dialog;
                // the page it opens knows nothing of this one.
                UiRequest::OpenLink(address) => web_sys::window()
                    .and_then(|w| w.open_with_url_and_target_and_features(&address, "_blank", "noopener,noreferrer").ok())
                    .is_none()
                    .then(|| "The browser did not open the link".to_string()),
                UiRequest::Quit => None,
                _ => Some("This is not done in a browser yet: open the desktop app for it".to_string()),
            };
            if let Some(said) = said {
                self.ui.session.status = said;
            }
        }

        /// The files chosen and read since the last frame, opened.
        fn open_picked(&mut self) {
            let picked = std::mem::take(&mut *self.picked.borrow_mut());
            for (name, read) in picked {
                let opened = read.and_then(|data| self.ui.session.open_bytes(&name, data).map(|_| ()).map_err(|e| e.to_string()));
                if let Err(e) = opened {
                    self.ui.session.status = format!("{name} could not be opened: {e}");
                }
            }
        }

        /// The settings put in the browser's storage when they are not what it has, as they
        /// change and not only when the page goes. Returns true when a change waits.
        fn keep_settings(&mut self) -> bool {
            let now = self.ui.session.settings(Some(&self.ui.settings()));
            if now == self.kept {
                return false;
            }
            if self.written.is_some_and(|at| at.elapsed() < KEEP_EVERY) {
                return true;
            }
            self.written = Some(web_time::Instant::now());
            let kept = serde_json::to_string(&now).is_ok_and(|text| storage().is_some_and(|s| s.set_item(SETTINGS_KEY, &text).is_ok()));
            if !kept && !self.keep_failed {
                self.keep_failed = true;
                self.ui.session.status = "Settings cannot be kept in this browser: its storage is off or full".to_string();
            }
            // Not tried again until they change once more.
            self.kept = now;
            false
        }
    }

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten())
    }

    /// Ask the browser for files of these kinds; each is read into memory and put in `picked`.
    fn pick(picked: &Picked, accept: &str, ctx: &egui::Context) -> Result<(), String> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or("the page has no document")?;
        let input: web_sys::HtmlInputElement =
            document.create_element("input").ok().and_then(|e| e.dyn_into().ok()).ok_or("the page cannot ask for a file")?;
        input.set_type("file");
        input.set_accept(accept);
        input.set_multiple(true);
        let (picked, ctx, chooser) = (Rc::clone(picked), ctx.clone(), input.clone());
        let chosen = Closure::once_into_js(move || {
            let Some(files) = chooser.files() else { return };
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                let name = file.name();
                if file.size() > MAX_OPENED {
                    picked.borrow_mut().push((name, Err("it is larger than a browser can hold; open it in the desktop app".to_string())));
                    continue;
                }
                let (picked, ctx) = (Rc::clone(&picked), ctx.clone());
                let reading = wasm_bindgen_futures::JsFuture::from(file.array_buffer());
                wasm_bindgen_futures::spawn_local(async move {
                    let read = match reading.await {
                        Ok(buffer) => Ok(js_sys::Uint8Array::new(&buffer).to_vec()),
                        Err(_) => Err("the browser could not read it".to_string()),
                    };
                    picked.borrow_mut().push((name, read));
                    ctx.request_repaint();
                });
            }
        });
        input.set_onchange(Some(chosen.unchecked_ref()));
        input.click();
        Ok(())
    }

    /// Hand `data` to the browser as a download named `name`.
    fn download(name: &str, data: &[u8], kind: &str) -> Result<(), String> {
        let window = web_sys::window().ok_or("the page has no window")?;
        let document = window.document().ok_or("the page has no document")?;
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(data));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(kind);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(|_| "the browser could not hold the file")?;
        let address = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| "the browser could not hold the file")?;
        let link: web_sys::HtmlAnchorElement =
            document.create_element("a").ok().and_then(|e| e.dyn_into().ok()).ok_or("the page cannot give a download")?;
        link.set_href(&address);
        link.set_download(name);
        link.click();
        // The address is let go once the browser has had time to take the file.
        let gone = Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&address);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(gone.unchecked_ref(), DOWNLOAD_KEPT_MS);
        Ok(())
    }

    /// The session the page shows: the sample mailbox, in the browser's time zone.
    fn session() -> Session {
        let mut session = Session::new();
        session.set_system_zone(openmailclient_ui_egui::local_zone());
        if let Err(e) = session.run("file.openSample", &Value::Null) {
            session.status = e.to_string();
        }
        session
    }

    fn app() -> App {
        let mut ui = MailUi::new(session());
        // What was kept last time, as on the desktop; settings that make no sense are left out.
        let text = storage().and_then(|s| s.get_item(SETTINGS_KEY).ok().flatten());
        if let Some(text) = text
            && let Err(e) = ui.execute("settings.load", &json!({ "text": text }))
        {
            ui.session.status = format!("The settings kept in this browser could not be read: {e}");
        }
        App { ui, started: web_time::Instant::now(), picked: Rc::default(), kept: Value::Null, written: None, keep_failed: false }
    }

    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id(super::CANVAS))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            let Some(canvas) = canvas else {
                web_sys::console::error_1(&"openmailclient: the page has no canvas for the window".into());
                return;
            };
            let creator =
                |_cc: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> { Ok(Box::new(app())) };
            if let Err(e) = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(creator)).await {
                web_sys::console::error_1(&e);
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

/// Built for this machine, it says where it runs instead.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("openmailclient-web runs in a web browser: build it with `cargo xtask web` and open target/web/index.html through a web server");
}
