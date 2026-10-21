//! Приложение `letter`: открывает главное окно редактора.
//!
//! `letter файл.docx` сразу открывает файл.

// В релизной сборке под Windows не показываем окно консоли рядом с программой.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod view;

use std::path::PathBuf;

use eframe::egui;
use letter_layout::FontLibrary;

use app::{APP_NAME, LetterApp};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |_cc| {
            let mut app = LetterApp::new(FontLibrary::new());
            if let Some(path) = file
                && let Err(error) = app.open(&path)
            {
                eprintln!("{error}");
            }
            Ok(Box::new(app))
        }),
    )
}
