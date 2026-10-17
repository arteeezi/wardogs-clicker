#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod clicker;
mod hotkey;
mod notification;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Clicker")
            .with_inner_size([360.0, 250.0])
            .with_resizable(false),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "clicker",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(&cc.egui_ctx)?))),
    )
}
