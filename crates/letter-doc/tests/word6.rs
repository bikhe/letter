//! Чтение старых .doc Word 6.0 и Word 95 (`tests/fixtures/doc/word6/`,
//! файлы из тестов Apache POI). Ожидаемый текст сверен с тем, что
//! показывает LibreOffice.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_doc::{is_doc, read, read_file};
use letter_model::{Alignment, Block, Color, Document, Inline, Run};

const FIXTURES: &[&str] = &[
    "word6",
    "formatting",
    "sections",
    "headers",
    "czech",
    "russian",
    "tables",
    "unicode",
];

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/doc/word6/{name}.doc"))
}

fn doc(name: &str) -> Document {
    read_file(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Непустые строки текста.
fn lines(doc: &Document) -> Vec<String> {
    doc.plain_text()
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

fn runs(doc: &Document) -> Vec<&Run> {
    doc.sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .flat_map(|p| &p.content)
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r),
            _ => None,
        })
        .collect()
}

#[test]
fn all_files_open() {
    for name in FIXTURES {
        let bytes = std::fs::read(path(name)).unwrap();
        assert!(is_doc(&bytes), "{name}");
        assert!(!doc(name).plain_text().trim().is_empty(), "{name}");
    }
}

#[test]
fn plain_text() {
    assert_eq!(
        lines(&doc("word6")),
        ["The quick brown fox jumps over the lazy dog"]
    );
    assert_eq!(
        lines(&doc("formatting")),
        [
            "The quick brown fox jumps over the lazy dog",
            "Paragraph 2",
            "Paragraph 3. Has some RED text and some BLUE BOLD text in it.",
            "Last (4th) paragraph.",
        ]
    );
}

#[test]
fn text_in_code_pages_of_fonts() {
    // Кодировка — по набору знаков шрифта: центральноевропейский
    // и кириллица.
    assert_eq!(lines(&doc("czech")), ["4 skóre a před 7 lety"]);
    assert_eq!(lines(&doc("russian")), ["Четыре балла и семь лет"]);
    let tables = lines(&doc("tables"));
    assert_eq!(tables[0], "Компания Аскат М");
    assert!(tables.iter().any(|l| l.contains("Алиготе, белое п/с")));
}

#[test]
fn unicode_pieces() {
    // Этот файл записан не самим Word: куски текста в UTF-16.
    let text = lines(&doc("unicode"));
    assert_eq!(text[0], "By Post and Fax (5555 5555)");
    assert!(text.iter().any(|l| l.starts_with("Attention: xx xxxxxxx")));
}

#[test]
fn character_and_paragraph_formatting() {
    let doc = doc("formatting");
    let all = runs(&doc);
    let red = all.iter().find(|r| r.plain_text() == "RED").unwrap();
    assert_eq!(red.props.color, Some(Color::Rgb(255, 0, 0)));
    assert_ne!(red.props.bold, Some(true));
    let blue = all.iter().find(|r| r.plain_text() == "BLUE BOLD").unwrap();
    assert_eq!(blue.props.color, Some(Color::Rgb(0, 0, 255)));
    assert_eq!(blue.props.bold, Some(true));
    assert_eq!(all[0].props.fonts.ascii.as_deref(), Some("Times New Roman"));

    let doc = self::doc("headers");
    let title = runs(&doc)
        .into_iter()
        .find(|r| r.plain_text().starts_with("STATEMENT"))
        .unwrap();
    assert_eq!(title.props.bold, Some(true));
    assert_eq!(title.props.size.map(|s| s.0), Some(28));
    let Some(Block::Paragraph(first)) = doc.sections[0].blocks.first() else {
        panic!("нет абзаца");
    };
    assert_eq!(first.props.alignment, Some(Alignment::Center));
}

#[test]
fn sections_and_page_size() {
    let doc = doc("sections");
    assert_eq!(doc.sections.len(), 3);
    assert_eq!(lines(&doc), ["This is a test.", "This is a new section."]);
    // Лист A4 и поля 3 см, как показывает LibreOffice.
    let props = &self::doc("headers").sections[0].props;
    let page = props.page_size.unwrap();
    assert_eq!((page.width.0, page.height.0), (11907, 16840));
    assert_eq!(props.margins.unwrap().left.0, 1701);
}

#[test]
fn headers_and_footers() {
    let doc = doc("headers");
    let props = &doc.sections[0].props;
    // Нижний колонтитул — поле «имя файла» с последним значением.
    let footer = props.footers.default.as_ref().unwrap();
    let text: String = footer.blocks.iter().map(Block::plain_text).collect();
    assert_eq!(text.trim(), r"g:\1998\publicat\sips\sip10sco.doc");
    assert!(props.headers.default.is_some());
}

#[test]
fn tables() {
    let doc = doc("tables");
    let tables: Vec<_> = doc.sections[0]
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    // Столько же таблиц и строк показывает LibreOffice.
    let rows: Vec<usize> = tables.iter().map(|t| t.rows.len()).collect();
    assert_eq!(rows, [21, 12, 4, 11]);
    let cell = &tables[0].rows[1].cells[0];
    assert!(
        cell.blocks
            .iter()
            .any(|b| b.plain_text().contains("Наименование"))
    );
}

#[test]
fn survive_saving_as_docx() {
    for name in FIXTURES {
        let doc = doc(name);
        let bytes = letter_docx::write_bytes(&doc).unwrap();
        let back = letter_docx::read_bytes(&bytes).unwrap();
        assert_eq!(lines(&back), lines(&doc), "{name}");
    }
}

#[test]
fn broken_files_are_errors_not_crashes() {
    for name in ["formatting", "headers", "tables", "unicode"] {
        let bytes = std::fs::read(path(name)).unwrap();
        for len in (0..bytes.len()).step_by(1499) {
            let _ = read(&bytes[..len]);
        }
        for at in (0..bytes.len()).step_by((bytes.len() / 400).max(53)) {
            let mut broken = bytes.clone();
            broken[at] ^= 0xA5;
            let _ = read(&broken);
        }
    }
}
