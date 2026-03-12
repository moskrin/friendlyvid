mod app;
mod commands;
mod media;
mod model;
mod state;
mod ui;
mod util;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    gstreamer::init().expect("failed to initialize GStreamer");
    gstreamer_editing_services::init().expect("failed to initialize GES");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("FriendlyVid"),
        ..Default::default()
    };

    eframe::run_native(
        "FriendlyVid",
        options,
        Box::new(|cc| Ok(Box::new(app::FriendlyVidApp::new(cc)))),
    )
}
