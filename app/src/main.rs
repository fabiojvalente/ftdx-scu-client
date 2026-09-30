mod app;
#[cfg(not(target_arch = "wasm32"))]
mod cat_server;
mod layout;
mod logging;
mod meter;
mod shortcuts;
mod theme;
mod waterfall;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use eframe::egui;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let initial = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let (filter, handle) = tracing_subscriber::reload::Layer::new(initial);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();
    logging::install(Box::new(move |level: &str| {
        let _ = handle.reload(tracing_subscriber::EnvFilter::new(level));
    }));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("SCU-LAN10 Client"),
        ..Default::default()
    };

    eframe::run_native(
        "SCU-LAN10 Client",
        options,
        Box::new(|cc| Ok(Box::new(app::ScuApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}

/// Browser entry point, invoked automatically by the wasm-bindgen glue.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();

    wasm_bindgen_futures::spawn_local(async {
        use wasm_bindgen::JsCast;

        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("scu_canvas")
            .expect("missing <canvas id=\"scu_canvas\">");
        let canvas: web_sys::HtmlCanvasElement =
            canvas.dyn_into().expect("#scu_canvas is not a canvas");

        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(app::ScuApp::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
}
