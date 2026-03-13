mod app;
mod commands;
mod media;
mod model;
mod state;
mod ui;
mod util;

const APP_ID: &str = "friendlyvid";
const ICON_PNG: &[u8] = include_bytes!("../friendlyvid.png");

fn install_desktop_entry() {
    use image::imageops::FilterType;

    let home = std::env::var("HOME").expect("HOME not set");
    let exe = std::env::current_exe()
        .expect("could not determine executable path")
        .canonicalize()
        .expect("could not canonicalize executable path");

    let img = image::load_from_memory(ICON_PNG).expect("failed to load icon");

    // Install icons at multiple sizes for title bar, taskbar, and launcher
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let icon_dir = format!("{home}/.local/share/icons/hicolor/{size}x{size}/apps");
        std::fs::create_dir_all(&icon_dir).expect("could not create icon directory");
        let icon_path = format!("{icon_dir}/{APP_ID}.png");
        let resized = img.resize_exact(size, size, FilterType::Lanczos3);
        resized.save(&icon_path).expect("could not write icon");
        println!("Installed icon: {icon_path}");
    }

    // Install .desktop file
    let apps_dir = format!("{home}/.local/share/applications");
    std::fs::create_dir_all(&apps_dir).expect("could not create applications directory");
    let desktop_path = format!("{apps_dir}/{APP_ID}.desktop");
    let desktop_contents = format!(
        "[Desktop Entry]\n\
         Name=FriendlyVid\n\
         Comment=Simple video editor\n\
         Exec={exe}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Type=Application\n\
         Categories=AudioVideo;Video;\n\
         StartupWMClass={APP_ID}\n",
        exe = exe.display()
    );
    std::fs::write(&desktop_path, desktop_contents).expect("could not write desktop file");
    println!("Installed desktop entry: {desktop_path}");

    // Update desktop database if available
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("-f")
        .arg("-t")
        .arg(format!("{home}/.local/share/icons/hicolor"))
        .status();

    println!("Done! You may need to log out and back in for the icon to appear.");
}

fn main() -> eframe::Result {
    // Handle --install before initializing anything heavy
    if std::env::args().any(|a| a == "--install") {
        install_desktop_entry();
        return Ok(());
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    gstreamer::init().expect("failed to initialize GStreamer");
    gstreamer_editing_services::init().expect("failed to initialize GES");

    let icon = {
        let img = image::load_from_memory(ICON_PNG).expect("failed to load icon").into_rgba8();
        let (w, h) = img.dimensions();
        egui::IconData {
            rgba: img.into_raw(),
            width: w,
            height: h,
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("FriendlyVid")
            .with_app_id(APP_ID)
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(app::FriendlyVidApp::new(cc)))),
    )
}
