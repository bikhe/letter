//! Просмотр свёрстанного документа: листы друг под другом на сером
//! фоне, прокрутка и масштаб.

use eframe::egui::{Pos2, Rect, ScrollArea, Ui, Vec2};
use letter_layout::{DocumentLayout, FontLibrary};
use letter_render::{DESK_COLOR, PagePainter, SCREEN_PIXELS_PER_POINT};

/// Наименьший масштаб (10 %).
pub const MIN_ZOOM: f32 = 0.1;

/// Наибольший масштаб (500 %).
pub const MAX_ZOOM: f32 = 5.0;

/// Промежуток между листами и вокруг них, точки экрана.
pub const PAGE_GAP: f32 = 16.0;

/// Масштаб в допустимых пределах.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// Масштаб, при котором самый широкий лист занимает ширину `width`
/// (за вычетом промежутков по краям).
pub fn fit_width_zoom(layout: &DocumentLayout, width: f32) -> f32 {
    let widest = layout.pages.iter().map(|p| p.width).fold(0.0, f32::max);
    if widest <= 0.0 {
        return 1.0;
    }
    clamp_zoom((width - 2.0 * PAGE_GAP) / (widest * SCREEN_PIXELS_PER_POINT))
}

/// Документ на экране.
#[derive(Debug)]
pub struct DocumentView {
    layout: DocumentLayout,
    zoom: f32,
    fit_width: bool,
    /// Прокрутка в прошлом кадре.
    offset: Vec2,
    /// Прокрутка, которую надо поставить в следующем кадре.
    scroll_to: Option<Vec2>,
    painter: PagePainter,
}

impl DocumentView {
    /// Документ при масштабе 100 %, прокрученный в начало.
    pub fn new(layout: DocumentLayout) -> DocumentView {
        DocumentView {
            layout,
            zoom: 1.0,
            fit_width: false,
            offset: Vec2::ZERO,
            scroll_to: None,
            painter: PagePainter::new(),
        }
    }

    /// Свёрстанный документ.
    pub fn layout(&self) -> &DocumentLayout {
        &self.layout
    }

    /// Масштаб: 1.0 — 100 %.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Включён режим «по ширине страницы».
    pub fn fits_width(&self) -> bool {
        self.fit_width
    }

    /// Задаёт масштаб (в пределах 10–500 %) и выключает «по ширине
    /// страницы».
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = clamp_zoom(zoom);
        self.fit_width = false;
    }

    /// Включает «по ширине страницы»: масштаб подстраивается под ширину
    /// окна, пока его не зададут вручную.
    pub fn fit_page_width(&mut self) {
        self.fit_width = true;
    }

    /// Размер листа на экране, точки.
    fn page_size(&self, index: usize) -> Vec2 {
        self.layout.pages.get(index).map_or(Vec2::ZERO, |page| {
            Vec2::new(page.width, page.height) * SCREEN_PIXELS_PER_POINT * self.zoom
        })
    }

    /// Размер всего содержимого (листы с промежутками), точки.
    pub fn content_size(&self) -> Vec2 {
        let mut size = Vec2::new(0.0, PAGE_GAP);
        for i in 0..self.layout.pages.len() {
            let page = self.page_size(i);
            size.x = size.x.max(page.x);
            size.y += page.y + PAGE_GAP;
        }
        size.x += 2.0 * PAGE_GAP;
        size
    }

    /// Места листов, если левый верхний угол содержимого в `origin`,
    /// а ширина видимой части `width`: листы по центру, друг под другом.
    pub fn page_rects(&self, origin: Pos2, width: f32) -> Vec<Rect> {
        let total = width.max(self.content_size().x);
        let mut top = origin.y + PAGE_GAP;
        (0..self.layout.pages.len())
            .map(|i| {
                let size = self.page_size(i);
                let left = origin.x + ((total - size.x) / 2.0).max(PAGE_GAP);
                let rect = Rect::from_min_size(Pos2::new(left, top), size);
                top += size.y + PAGE_GAP;
                rect
            })
            .collect()
    }

    /// Меняет масштаб в `factor` раз так, чтобы точка под указателем
    /// (`anchor` — от левого верхнего угла видимой части) осталась на месте.
    fn zoom_around(&mut self, factor: f32, anchor: Vec2) {
        let old = self.zoom;
        let new = clamp_zoom(old * factor);
        if new == old {
            return;
        }
        let point = (self.offset + anchor) * (new / old);
        self.set_zoom(new);
        self.offset = (point - anchor).max(Vec2::ZERO);
        self.scroll_to = Some(self.offset);
    }

    /// Рисует документ на всё доступное место: серый фон, листы,
    /// полосы прокрутки. Ctrl + колесо мыши меняет масштаб.
    pub fn ui(&mut self, ui: &mut Ui, library: &FontLibrary) {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0.0, DESK_COLOR);
        if self.fit_width {
            self.zoom = fit_width_zoom(&self.layout, area.width());
        }
        let (zoom, pointer) = ui.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        if let Some(pointer) = pointer
            && zoom != 1.0
            && area.contains(pointer)
        {
            self.zoom_around(zoom, pointer - area.min);
        }

        let mut scroll = ScrollArea::both().auto_shrink(false);
        if let Some(offset) = self.scroll_to.take() {
            scroll = scroll.scroll_offset(offset);
        }
        let content = self.content_size();
        let output = scroll.show_viewport(ui, |ui, _viewport| {
            let width = ui.available_width();
            ui.set_min_size(Vec2::new(content.x.max(width), content.y));
            let origin = ui.min_rect().min;
            let clip = ui.clip_rect();
            for (page, rect) in self.layout.pages.iter().zip(self.page_rects(origin, width)) {
                if rect.expand(PAGE_GAP).intersects(clip) {
                    self.painter.paint_page(ui.painter(), library, page, rect);
                }
            }
        });
        self.offset = output.state.clamped_offset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    use letter_layout::Shaper;

    fn layout(name: &str, library: &FontLibrary) -> DocumentLayout {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/docx")
            .join(name);
        let doc = letter_docx::read_file(path).unwrap();
        letter_layout::layout_document(&doc, &mut Shaper::new(), library)
    }

    #[test]
    fn zoom_stays_within_limits() {
        assert_eq!(clamp_zoom(0.01), MIN_ZOOM);
        assert_eq!(clamp_zoom(7.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(1.5), 1.5);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
    }

    #[test]
    fn pages_stack_with_gaps_and_center() {
        let library = FontLibrary::embedded_only();
        let mut view = DocumentView::new(layout("lists.docx", &library));
        view.layout.pages.push(view.layout.pages[0].clone());
        // Letter: 612 × 792 пт = 816 × 1056 точек при 100 %.
        let size = view.content_size();
        assert_eq!(size, Vec2::new(816.0 + 32.0, 16.0 + 2.0 * (1056.0 + 16.0)));
        let rects = view.page_rects(Pos2::ZERO, 1000.0);
        assert_eq!(rects[0].min, Pos2::new(92.0, 16.0));
        assert_eq!(rects[1].min, Pos2::new(92.0, 16.0 + 1056.0 + 16.0));
        // Окно уже листа: лист прижат к левому краю с промежутком.
        assert_eq!(view.page_rects(Pos2::ZERO, 300.0)[0].min.x, PAGE_GAP);

        view.set_zoom(0.5);
        assert_eq!(view.page_rects(Pos2::ZERO, 1000.0)[0].width(), 408.0);
    }

    #[test]
    fn fit_width_follows_the_window() {
        let library = FontLibrary::embedded_only();
        let doc = layout("lists.docx", &library);
        assert!((fit_width_zoom(&doc, 816.0 + 32.0) - 1.0).abs() < 1e-5);
        assert_eq!(fit_width_zoom(&doc, 10.0), MIN_ZOOM);
        assert_eq!(
            fit_width_zoom(&DocumentLayout { pages: vec![] }, 500.0),
            1.0
        );

        let mut view = DocumentView::new(doc);
        view.fit_page_width();
        assert!(view.fits_width());
        run(&mut view, &library, Vec::new());
        // Окно в тесте — 800 точек шириной.
        assert!((view.zoom() - (800.0 - 32.0) / 816.0).abs() < 1e-3);
        view.set_zoom(2.0);
        assert!(!view.fits_width());
    }

    #[test]
    fn ctrl_wheel_zooms_around_the_pointer() {
        let library = FontLibrary::embedded_only();
        let mut view = DocumentView::new(layout("headings.docx", &library));
        run(&mut view, &library, Vec::new());
        let events = vec![
            egui::Event::PointerMoved(Pos2::new(400.0, 300.0)),
            egui::Event::Zoom(2.0),
        ];
        run(&mut view, &library, events);
        assert_eq!(view.zoom(), 2.0);
        // Точка под указателем осталась на месте: прокрутка — на столько
        // же, на сколько точка отъехала от угла.
        assert_eq!(view.scroll_to, None);
        assert_eq!(view.offset, Vec2::new(400.0, 300.0));

        // Дальше 500 % не увеличивается.
        view.set_zoom(4.0);
        view.zoom_around(10.0, Vec2::ZERO);
        assert_eq!(view.zoom(), MAX_ZOOM);
    }

    /// Один кадр в окне 800 × 600 без видеокарты.
    fn run(view: &mut DocumentView, library: &FontLibrary, events: Vec<egui::Event>) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| view.ui(ui, library));
        });
        output.textures_delta.clear();
    }
}
