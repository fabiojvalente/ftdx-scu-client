mod app;
mod waterfall;

use eframe::egui;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
