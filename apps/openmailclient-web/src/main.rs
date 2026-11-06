//! OpenMailClient in a web browser (M6): the window the desktop app shows, built for
//! WebAssembly and drawn on a canvas with WebGL. `cargo xtask web` builds it into `target/web`,
//! with the page that starts it.
//!
//! In the browser there are no files, no threads and no sockets: the sample mailbox is opened,
//! what would run on a thread of its own runs where it is asked for, and nothing reaches beyond
//! the page. Opening data files from the browser and keeping settings in it come next (see
//! `ROADMAP.md`, Do later).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// The id of the canvas the page gives the window.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const CANVAS: &str = "openmailclient";

#[cfg(target_arch = "wasm32")]
mod web {
    use openmailclient_engine::Session;
    use openmailclient_ui_egui::MailUi;
    use serde_json::Value;

    struct App {
        ui: MailUi,
        /// When the page started, by the browser's clock: what is due is counted from it.
        started: web_time::Instant,
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.ui.ui(ui);
            let ctx = ui.ctx().clone();
            self.ui.session.tick_clock();
            if let Some(left) = self.ui.session.reminders_due(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)) {
                ctx.request_repaint_after(std::time::Duration::from_millis(left));
            }
        }
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

    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            use eframe::wasm_bindgen::JsCast as _;
            let canvas = eframe::web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id(super::CANVAS))
                .and_then(|e| e.dyn_into::<eframe::web_sys::HtmlCanvasElement>().ok());
            let Some(canvas) = canvas else {
                eframe::web_sys::console::error_1(&"openmailclient: the page has no canvas for the window".into());
                return;
            };
            let creator = |_cc: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Box::new(App { ui: MailUi::new(session()), started: web_time::Instant::now() }))
            };
            if let Err(e) = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(creator)).await {
                eframe::web_sys::console::error_1(&e);
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
