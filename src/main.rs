mod app;
mod assets;
mod config;
mod exp;
mod parser;
mod zone;

use app::PoELevelingGuideApp;

fn load_icon() -> Option<eframe::egui::IconData> {
    let icon_bytes = include_bytes!("../icon.png");
    let img = image::load_from_memory(icon_bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some(eframe::egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

fn main() -> eframe::Result<()> {
    // Setup logging
    env_logger::init();

    // Single fullscreen transparent overlay window
    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_maximized(true)
        .with_mouse_passthrough(false);

    if let Some(icon) = load_icon() {
        vp = vp.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport: vp,
        multisampling: 0,
        ..Default::default()
    };

    eframe::run_native(
        "PoE Leveling Guide",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(PoELevelingGuideApp::new(cc)))
        }),
    )
}
