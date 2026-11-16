// 릴리스 빌드에서는 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod elevate;
mod persist;
mod route;
mod selection;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(app::APP_TITLE)
            .with_inner_size([1080.0, 640.0])
            .with_min_inner_size([760.0, 420.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        app::APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
