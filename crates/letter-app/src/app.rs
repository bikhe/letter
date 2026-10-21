//! Главное окно: меню, документ, строка состояния.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, Ui, ViewportCommand};
use letter_layout::{FontLibrary, Shaper, layout_document};
use letter_model::Document;

use crate::view::{DocumentView, MAX_ZOOM, MIN_ZOOM};

/// Название программы в заголовке окна.
pub const APP_NAME: &str = "letter";

/// Сочетание клавиш «Открыть».
const OPEN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);

/// Состояние приложения.
pub struct LetterApp {
    library: FontLibrary,
    shaper: Shaper,
    view: DocumentView,
    path: Option<PathBuf>,
    error: Option<String>,
    /// Заголовок окна, уже показанный системе.
    shown_title: String,
}

impl LetterApp {
    /// Окно с новым пустым документом.
    pub fn new(library: FontLibrary) -> LetterApp {
        let mut shaper = Shaper::new();
        let layout = layout_document(&Document::new(), &mut shaper, &library);
        LetterApp {
            library,
            shaper,
            view: DocumentView::new(layout),
            path: None,
            error: None,
            shown_title: String::new(),
        }
    }

    /// Открывает файл .docx. При ошибке открытый документ не меняется.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let doc = letter_docx::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let layout = layout_document(&doc, &mut self.shaper, &self.library);
        self.view = DocumentView::new(layout);
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    /// Открывает файл и показывает ошибку, если не вышло.
    fn open_or_report(&mut self, path: &Path) {
        if let Err(error) = self.open(path) {
            self.error = Some(error);
        }
    }

    /// Заголовок окна: имя файла и название программы.
    pub fn title(&self) -> String {
        match self.path.as_deref().and_then(Path::file_name) {
            Some(name) => format!("{} — {APP_NAME}", name.to_string_lossy()),
            None => format!("Новый документ — {APP_NAME}"),
        }
    }

    /// Документ на экране.
    #[cfg(test)]
    pub fn view(&self) -> &DocumentView {
        &self.view
    }

    /// Последняя ошибка, которую видит пользователь.
    #[cfg(test)]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn pick_and_open(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("Документ Word", &["docx"])
            .add_filter("Все файлы", &["*"])
            .pick_file();
        if let Some(path) = picked {
            self.open_or_report(&path);
        }
    }

    fn menu(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Файл", |ui| {
                let open = egui::Button::new("Открыть…")
                    .shortcut_text(ui.ctx().format_shortcut(&OPEN_SHORTCUT));
                if ui.add(open).clicked() {
                    self.pick_and_open();
                }
                ui.separator();
                if ui.button("Выход").clicked() {
                    ui.send_viewport_cmd(ViewportCommand::Close);
                }
            });
            ui.menu_button("Вид", |ui| {
                if ui.button("Масштаб 100 %").clicked() {
                    self.view.set_zoom(1.0);
                }
                if ui.button("По ширине страницы").clicked() {
                    self.view.fit_page_width();
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(format!("Страниц: {}", self.view.layout().pages.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut percent = (self.view.zoom() * 100.0).round();
                ui.label(format!("{percent} %"));
                if ui.small_button("+").clicked() {
                    self.view.set_zoom(self.view.zoom() + 0.1);
                }
                let slider = egui::Slider::new(&mut percent, MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0)
                    .logarithmic(true)
                    .show_value(false);
                if ui.add(slider).changed() {
                    self.view.set_zoom(percent / 100.0);
                }
                if ui.small_button("−").clicked() {
                    self.view.set_zoom(self.view.zoom() - 0.1);
                }
                let fit = ui.selectable_label(self.view.fits_width(), "По ширине");
                if fit.clicked() {
                    self.view.fit_page_width();
                }
            });
        });
    }
}

impl eframe::App for LetterApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if ui.input_mut(|i| i.consume_shortcut(&OPEN_SHORTCUT)) {
            self.pick_and_open();
        }
        let dropped = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .find_map(|f| Some(f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()))
        });
        if let Some(path) = dropped {
            self.open_or_report(&path);
        }
        let title = self.title();
        if title != self.shown_title {
            ui.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.view.ui(ui, &self.library));

        if let Some(error) = self.error.clone() {
            egui::Window::new("Не удалось открыть файл")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label(error);
                    if ui.button("ОК").clicked() {
                        self.error = None;
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/docx")
            .join(name)
    }

    #[test]
    fn starts_with_a_blank_page_and_opens_files() {
        let mut app = LetterApp::new(FontLibrary::embedded_only());
        assert_eq!(app.view().layout().pages.len(), 1);
        assert_eq!(app.title(), "Новый документ — letter");

        app.open(&fixture("headings.docx")).unwrap();
        assert_eq!(app.title(), "headings.docx — letter");
        assert!(!app.view().layout().pages[0].lines.is_empty());
    }

    #[test]
    fn open_errors_keep_the_document() {
        let mut app = LetterApp::new(FontLibrary::embedded_only());
        app.open(&fixture("headings.docx")).unwrap();
        app.open_or_report(Path::new("нет-такого-файла.docx"));
        assert!(app.error().unwrap().starts_with("нет-такого-файла.docx: "));
        assert_eq!(app.title(), "headings.docx — letter");
    }
}
