//! Starts logging, the core and the window.

// A release build on Windows opens no console window next to the app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use project_transfer::core::AppCore;
use project_transfer::store::Store;
use project_transfer::ui::{App, RfdPicker, theme};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let store = Store::open_default()?;
    // Keeping the guard alive keeps the log file writer flushing.
    let _log = project_transfer::logging::init(&store.logs_dir())?;
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .map_err(|e| anyhow::anyhow!("the built-in icon is not a valid PNG: {e}"))?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Project Transfer")
            .with_icon(icon)
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Project Transfer",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let core = AppCore::start(store, Some(cc.egui_ctx.clone()))?;
            Ok(Box::new(App::new(Arc::new(core), Box::new(RfdPicker))))
        }),
    )
    .map_err(|e| anyhow::anyhow!("the window could not open: {e}"))
}
