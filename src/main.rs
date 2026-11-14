mod app;
mod audio;
mod platform;
mod speech;

use app::SpikeApp;
use platform::flags::OVERLAY_TITLE;
use platform::version::{is_supported, os_build};

fn main() {
    env_logger::init();
    let build = os_build();
    if !is_supported(build) {
        eprintln!(
            "CuePal needs Windows 10 build 19041 or later (found build {build}). Not launching."
        );
        std::process::exit(1);
    }
    log::info!("cuepal m0 scaffold ok (os build {build})");

    // Watcher thread: discover the overlay HWND once it exists, apply flags.
    std::thread::spawn(|| {
        for _ in 0..100 {
            if let Some(h) = platform::flags::find_overlay_hwnd() {
                unsafe {
                    let _ = platform::flags::apply_overlay_flags(h);
                }
                log::info!("overlay hwnd flags applied");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        log::warn!("overlay hwnd not found within 10s");
    });

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([420.0, 180.0])
            .with_resizable(true)
            .with_transparent(true)
            .with_always_on_top()
            .with_decorations(false)
            .with_taskbar(false)
            .with_active(false)
            .with_title(OVERLAY_TITLE),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "CuePal Overlay (spike)",
        options,
        Box::new(|cc| Ok(Box::new(SpikeApp::new(cc)))),
    );
}
