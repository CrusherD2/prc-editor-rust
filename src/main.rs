mod param_file;
mod param_types;
mod hash_labels;

// Desktop version with GUI
#[cfg(not(target_os = "horizon"))]
mod ui;

#[cfg(not(target_os = "horizon"))]
use eframe::egui;
#[cfg(not(target_os = "horizon"))]
use ui::PrcEditorApp;

// Nintendo Switch version
#[cfg(target_os = "horizon")]
use skyline::prelude::*;

#[cfg(not(target_os = "horizon"))]
fn load_app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("app icon PNG must be valid")
}

#[cfg(not(target_os = "horizon"))]
fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "PRC Editor",
        options,
        Box::new(|_cc| Ok(Box::new(PrcEditorApp::new()))),
    )
}

#[cfg(target_os = "horizon")]
fn main() {

    // TODO: Implement Switch-specific functionality
    // Could be a console interface or hooks into the game's parameter system
} 