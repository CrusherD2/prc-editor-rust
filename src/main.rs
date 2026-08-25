// Hide the extra console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod param_file;
mod param_types;
mod hash_labels;

// Desktop version with GUI
#[cfg(not(target_os = "horizon"))]
mod ui;
#[cfg(not(target_os = "horizon"))]
mod hash_crack;
#[cfg(not(target_os = "horizon"))]
mod flip;
#[cfg(not(target_os = "horizon"))]
mod flip_apply;
#[cfg(not(target_os = "horizon"))]
mod viewport;
#[cfg(not(target_os = "horizon"))]
mod flip_ui;
#[cfg(not(target_os = "horizon"))]
mod update;
#[cfg(all(windows, not(target_os = "horizon")))]
mod win_boot;

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
    #[cfg(windows)]
    win_boot::install_create_hook();

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_visible(false)
            .with_icon(load_app_icon()),
        window_builder: Some(Box::new(|builder| {
            // Restoring maximized calls ShowWindow before the first paint, which flashes white.
            builder.with_visible(false).with_maximized(false)
        })),
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(windows)]
            win_boot::swallow_white_erase(builder);
        })),
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                instance_descriptor: wgpu::InstanceDescriptor {
                    // Vulkan can spawn a helper HWND that flashes white on Windows.
                    backends: if cfg!(windows) {
                        wgpu::Backends::DX12
                    } else {
                        wgpu::Backends::PRIMARY
                    },
                    ..wgpu::InstanceDescriptor::new_without_display_handle()
                },
                power_preference: wgpu::PowerPreference::HighPerformance,
                device_descriptor: std::sync::Arc::new(|_adapter| wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::default() | ssbh_wgpu::REQUIRED_FEATURES,
                    required_limits: ssbh_wgpu::REQUIRED_LIMITS,
                    ..Default::default()
                }),
                display_handle: None,
                native_adapter_selector: None,
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    eframe::run_native(
        concat!("PRC Editor ", env!("CARGO_PKG_VERSION")),
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(
                egui::viewport::SystemTheme::Dark,
            ));
            #[cfg(windows)]
            win_boot::hide_until_first_frame(cc);
            Ok(Box::new(PrcEditorApp::new()))
        }),
    )
}

#[cfg(target_os = "horizon")]
fn main() {

    // TODO: Implement Switch-specific functionality
    // Could be a console interface or hooks into the game's parameter system
}
