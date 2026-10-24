//! Чтение .doc: файлы, которые LibreOffice сохранил из наших .docx
//! (`tests/fixtures/doc/`), сверяются с исходными .docx.

// В тестах падение на unwrap() и есть сигнал ошибки.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use letter_doc::{ReadError, is_doc, is_encrypted, read, read_file, read_with_password};
use letter_model::{
    Block, Border, Color, Document, HeaderFooter, HeaderFooters, Inline, ListCounters, NoteKind,
    Paragraph, Picture, RunContent, StyleKind, Table, Twips,
};

const FIXTURES: &[&str] = &[
    "plain-text",
    "bold-italic",
    "headings",
    "lists",
    "table",
    "ru-en",
    "pages",
    "header-footer",
    "headers",
    "notes",
    "comments",
    "sections",
    "borders",
    "cells",
    "links",
    "image",
    "floating",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn doc(name: &str) -> Document {
    read_file(dir().join(format!("doc/{name}.doc"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn docx(name: &str) -> Document {
    letter_docx::read_file(dir().join(format!("docx/{name}.docx"))).unwrap()
}

fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    doc.sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// Непустые строки текста: в .docx раздел кончается отдельным пустым
/// абзацем, а в .doc конец раздела — знак последнего абзаца.
fn lines(text: &str) -> Vec<&str> {
    text.lines().filter(|l| !l.is_empty()).collect()
}

#[test]
fn text_matches_the_original_docx() {
    for name in FIXTURES {
        // В `pages` LibreOffice превратил разрыв страницы в абзаце
        // в «с новой страницы» у этого абзаца.
        let expected = docx(name).plain_text().replace('\u{c}', "");
        assert_eq!(lines(&doc(name).plain_text()), lines(&expected), "{name}");
    }
}

#[test]
fn page_size_and_margins_match() {
    for name in FIXTURES {
        let (doc, docx) = (doc(name), docx(name));
        let props = &doc.sections[0].props;
        let original = &docx.sections[0].props;
        assert_eq!(props.page_size, original.page_size, "{name}");
        assert_eq!(
            props.margins.map(|m| (m.top, m.left, m.right, m.bottom)),
            original.margins.map(|m| (m.top, m.left, m.right, m.bottom)),
            "{name}"
        );
    }
}

#[test]
fn bold_and_italic() {
    let doc = doc("bold-italic");
    let runs: Vec<_> = paragraphs(&doc)
        .into_iter()
        .flat_map(|p| &p.content)
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r),
            _ => None,
        })
        .collect();
    let find = |text: &str| {
        runs.iter()
            .find(|r| r.content == [RunContent::Text(text.into())])
            .unwrap_or_else(|| panic!("{text}"))
    };
    assert_eq!(find("bold").props.bold, Some(true));
    assert_eq!(find("italic").props.italic, Some(true));
    assert_eq!(find("italic").props.bold, None);
}

#[test]
fn headings_have_their_styles() {
    let doc = doc("headings");
    let styles: Vec<_> = paragraphs(&doc)
        .iter()
        .map(|p| p.props.style.clone())
        .collect();
    assert_eq!(styles[0].as_deref(), Some("Title"));
    assert_eq!(styles[1].as_deref(), Some("Heading1"));
    assert_eq!(styles[2], None);
    assert_eq!(styles[3].as_deref(), Some("Heading2"));
    let heading = doc.styles.get("Heading1").unwrap();
    assert_eq!(heading.kind, StyleKind::Paragraph);
    assert_eq!(heading.name.as_deref(), Some("heading 1"));
    assert_eq!(heading.based_on.as_deref(), Some("Normal"));
    assert!(heading.run.size.is_some() || heading.run.bold.is_some());
    assert!(doc.styles.get("Normal").unwrap().is_default);
}

#[test]
fn table_with_merged_cell() {
    let doc = doc("table");
    let table = doc.sections[0]
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.grid, [Twips(2880); 3]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 3);
    let last = &table.rows[2].cells;
    assert_eq!(last.len(), 2);
    assert_eq!(last[0].props.grid_span, Some(2));
    assert_eq!(last[0].plain_text(), "Merged R3C1–C2");
    assert_eq!(
        paragraphs(&doc).last().map(|p| p.plain_text()).as_deref(),
        Some("Text after the table.")
    );
}

#[test]
fn chapters_start_on_new_pages() {
    let doc = doc("pages");
    let all = paragraphs(&doc);
    let i = all
        .iter()
        .position(|p| p.plain_text() == "Chapter 2")
        .unwrap();
    // Разрыв — у самой главы или у пустого абзаца перед ней.
    let before = |p: &Paragraph| p.props.page_break_before == Some(true);
    assert!(before(all[i]) || (all[i - 1].plain_text().is_empty() && before(all[i - 1])));
}

/// Текст колонтитулов раздела: обычный, первой страницы, чётных.
fn header_texts(h: &HeaderFooters) -> [Option<String>; 3] {
    let text = |h: &Option<HeaderFooter>| {
        h.as_ref().map(|h| {
            h.blocks
                .iter()
                .map(Block::plain_text)
                .collect::<Vec<_>>()
                .join("\n")
        })
    };
    [text(&h.default), text(&h.first), text(&h.even)]
}

/// Все особые знаки в тексте (ссылки на сноски и примечания).
fn references(doc: &Document) -> Vec<RunContent> {
    paragraphs(doc)
        .into_iter()
        .flat_map(|p| &p.content)
        .filter_map(|i| match i {
            Inline::Run(r) => Some(&r.content),
            _ => None,
        })
        .flatten()
        .filter(|c| {
            matches!(
                c,
                RunContent::NoteReference(_) | RunContent::CommentReference(_)
            )
        })
        .cloned()
        .collect()
}

#[test]
fn headers_and_footers() {
    let doc = doc("header-footer");
    let props = &doc.sections[0].props;
    assert_eq!(
        header_texts(&props.headers),
        [Some("Header text".into()), None, None]
    );
    assert_eq!(
        header_texts(&props.footers),
        [Some("Footer text".into()), None, None]
    );
}

#[test]
fn first_and_even_page_headers() {
    let (doc, docx) = (doc("headers"), docx("headers"));
    assert!(doc.settings.even_and_odd_headers);
    assert_eq!(doc.sections.len(), 2);
    let (first, second) = (&doc.sections[0].props, &doc.sections[1].props);
    assert!(first.title_page);
    assert!(!second.title_page);
    let original = &docx.sections[0].props;
    assert_eq!(
        header_texts(&first.headers),
        header_texts(&original.headers)
    );
    // В нижнем колонтитуле — поля номера страницы; их значения
    // LibreOffice пересчитал, поэтому сверяем начало.
    let footers = header_texts(&first.footers);
    assert!(
        footers[0]
            .as_deref()
            .is_some_and(|t| t.starts_with("Odd page footer, page"))
    );
    assert_eq!(footers[1].as_deref(), Some("First page footer"));
    assert_eq!(footers[2].as_deref(), Some("Even page footer"));
    // Во втором разделе свои нижние колонтитулы.
    let footers = header_texts(&second.footers);
    assert_eq!(footers[0].as_deref(), Some("Second section footer"));
    assert_eq!(footers[1], None);
    assert_eq!(footers[2].as_deref(), Some("Second section even footer"));
}

#[test]
fn footnotes_and_endnotes() {
    let (doc, docx) = (doc("notes"), docx("notes"));
    let texts = |notes: &[letter_model::Note]| {
        notes
            .iter()
            .map(|n| {
                let text: Vec<_> = n.blocks.iter().map(Block::plain_text).collect();
                (n.id.clone(), n.special.clone(), text.join("\n"))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&doc.footnotes), texts(&docx.footnotes));
    assert_eq!(texts(&doc.endnotes), texts(&docx.endnotes));
    assert_eq!(doc.footnotes.iter().filter(|n| n.is_regular()).count(), 2);
    assert_eq!(references(&doc), references(&docx));
    // Сноска начинается со своего номера.
    let first = doc.footnotes.iter().find(|n| n.is_regular()).unwrap();
    let Block::Paragraph(p) = &first.blocks[0] else {
        panic!("сноска без абзаца");
    };
    assert!(p.content.iter().any(|i| matches!(
        i,
        Inline::Run(r) if r.content.contains(&RunContent::NoteMark(NoteKind::Footnote))
    )));
}

#[test]
fn comments_with_authors_and_ranges() {
    let (doc, docx) = (doc("comments"), docx("comments"));
    let comments = |d: &Document| {
        d.comments
            .iter()
            .map(|c| {
                (
                    c.id.clone(),
                    c.author.clone(),
                    c.initials.clone(),
                    c.plain_text(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(comments(&doc), comments(&docx));
    assert_eq!(doc.comments.len(), 3);
    assert_eq!(references(&doc), references(&docx));
    // Начала и концы отмеченных кусков стоят там же, где в .docx.
    let marks = |d: &Document| {
        paragraphs(d)
            .into_iter()
            .flat_map(|p| &p.content)
            .filter(|i| matches!(i, Inline::CommentStart(_) | Inline::CommentEnd(_)))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(marks(&doc), marks(&docx));
}

#[test]
fn several_sections() {
    let (doc, docx) = (doc("sections"), docx("sections"));
    assert_eq!(doc.sections.len(), docx.sections.len());
    assert_eq!(lines(&doc.plain_text()), lines(&docx.plain_text()));
    // «С нечётной страницы» у последнего раздела LibreOffice в .doc не
    // сохраняет (заменяет стилем страницы), поэтому его не сверяем.
    let count = doc.sections.len() - 1;
    let pairs = doc.sections.iter().zip(&docx.sections).take(count);
    for (i, (a, b)) in pairs.enumerate() {
        assert_eq!(a.props.page_size, b.props.page_size, "раздел {i}");
        assert_eq!(
            a.props.start.unwrap_or_default(),
            b.props.start.unwrap_or_default(),
            "раздел {i}"
        );
        assert_eq!(
            a.props.columns.as_ref().map(|c| c.count),
            b.props.columns.as_ref().map(|c| c.count).or(Some(1)),
            "раздел {i}"
        );
    }
}

/// Номера пунктов списков у абзацев по порядку (`None` — не в списке).
fn list_labels(doc: &Document) -> Vec<(String, Option<String>)> {
    let mut counters = ListCounters::default();
    paragraphs(doc)
        .into_iter()
        .map(|p| {
            let label = doc
                .resolve_paragraph(&p.props)
                .numbering
                .and_then(|(id, level)| counters.next(&doc.numbering, id, level))
                .map(|label| label.text);
            (p.plain_text(), label)
        })
        .collect()
}

#[test]
fn list_numbers_and_bullets() {
    let (doc, docx) = (doc("lists"), docx("lists"));
    let labels = list_labels(&doc);
    assert_eq!(labels, list_labels(&docx));
    assert_eq!(labels[6].1.as_deref(), Some("2."));
    // Маркер — знак шрифта Symbol, как в .docx.
    let (id, level) = doc
        .resolve_paragraph(&paragraphs(&doc)[1].props)
        .numbering
        .unwrap();
    let bullet = doc.numbering.level(id, level).unwrap();
    assert_eq!(bullet.run.fonts.ascii.as_deref(), Some("Symbol"));
    assert!(bullet.paragraph.indentation.left.is_some());
}

/// Вид и толщина линии.
fn line(border: &Option<Border>) -> Option<(String, Option<u32>)> {
    border.as_ref().map(|b| (b.style.clone(), b.size))
}

#[test]
fn paragraph_borders_and_shading() {
    let (doc, docx) = (doc("borders"), docx("borders"));
    let (all, original) = (paragraphs(&doc), paragraphs(&docx));
    assert_eq!(all.len(), original.len());
    for (p, o) in all.iter().zip(&original) {
        assert_eq!(p.props.shading, o.props.shading, "{}", p.plain_text());
        assert_eq!(p.props.borders.is_some(), o.props.borders.is_some());
    }
    let frame = all[0].props.borders.as_ref().unwrap();
    let expected = original[0].props.borders.as_ref().unwrap();
    assert_eq!(frame.top, expected.top);
    assert_eq!(frame.left, expected.left);
    assert_eq!(frame.right, expected.right);
    // LibreOffice пишет рамку двух абзацев без линии между ними: верх
    // у первого, низ у второго.
    assert_eq!(
        all[1].props.borders.as_ref().unwrap().bottom,
        expected.bottom
    );
    let heading = all[2].props.borders.as_ref().unwrap();
    assert_eq!(line(&heading.bottom), Some(("double".into(), Some(6))));
    assert_eq!(heading.top, None);
}

fn first_table(doc: &Document) -> &Table {
    doc.sections[0]
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .unwrap()
}

#[test]
fn cell_borders_shading_and_row_heights() {
    let (doc, docx) = (doc("cells"), docx("cells"));
    let (table, original) = (first_table(&doc), first_table(&docx));
    assert_eq!(table.rows.len(), original.rows.len());
    for (row, o) in table.rows.iter().zip(&original.rows) {
        assert_eq!(row.props.height, o.props.height);
        assert_eq!(row.props.header, o.props.header);
        for (cell, o) in row.cells.iter().zip(&o.cells) {
            assert_eq!(cell.props.shading, o.props.shading);
            let (b, o) = (
                cell.props.borders.as_ref().unwrap(),
                o.props.borders.as_ref().unwrap(),
            );
            for (side, original) in [
                (&b.top, &o.top),
                (&b.left, &o.left),
                (&b.bottom, &o.bottom),
                (&b.right, &o.right),
            ] {
                assert_eq!(line(side), line(original));
            }
        }
    }
    let red = table.rows[0].cells[0].props.borders.as_ref().unwrap();
    assert_eq!(
        red.top.as_ref().and_then(|b| b.color),
        Some(Color::Rgb(0xC0, 0, 0))
    );
}

#[test]
fn hyperlinks_and_bookmarks() {
    let (doc, docx) = (doc("links"), docx("links"));
    let links = |d: &Document| {
        paragraphs(d)
            .into_iter()
            .flat_map(|p| &p.content)
            .filter_map(|i| match i {
                Inline::Run(r) => r
                    .props
                    .link
                    .as_ref()
                    .map(|l| (r.plain_text(), l.target.clone(), l.anchor.clone())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(links(&doc), links(&docx));
    assert_eq!(links(&doc).len(), 2);
    let bookmarks = |d: &Document| {
        paragraphs(d)
            .into_iter()
            .flat_map(|p| &p.content)
            .filter_map(|i| match i {
                Inline::BookmarkStart(b) => Some(b.name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(bookmarks(&doc), bookmarks(&docx));
    // Поля ссылок в тексте не остаются.
    assert!(
        !paragraphs(&doc)
            .iter()
            .any(|p| p.content.iter().any(|i| matches!(
                i,
                Inline::Run(r) if r.content.iter().any(|c| matches!(c, RunContent::FieldChar(_)))
            )))
    );
}

#[test]
fn document_properties() {
    let (doc, docx) = (doc("links"), docx("links"));
    let (core, original) = (&doc.properties.core, &docx.properties.core);
    assert_eq!(core.title.as_deref(), Some("Ссылки"));
    assert_eq!(core.title, original.title);
    assert_eq!(core.creator, original.creator);
    assert_eq!(core.created, original.created);
    assert_eq!(core.modified, original.modified);
}

/// Картинки документа по порядку.
fn pictures(doc: &Document) -> Vec<&Picture> {
    paragraphs(doc)
        .into_iter()
        .flat_map(|p| &p.content)
        .filter_map(|i| match i {
            Inline::Run(r) => Some(&r.content),
            _ => None,
        })
        .flatten()
        .filter_map(|c| match c {
            RunContent::Picture(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// Размеры почти равны: LibreOffice округляет их до twip.
fn close(a: i64, b: i64) -> bool {
    (a - b).abs() <= 1000
}

#[test]
fn inline_and_floating_pictures() {
    for name in ["image", "floating"] {
        let (doc, docx) = (doc(name), docx(name));
        let (all, original) = (pictures(&doc), pictures(&docx));
        assert_eq!(all.len(), original.len(), "{name}");
        for (p, o) in all.iter().zip(&original) {
            assert!(
                close(p.width, o.width) && close(p.height, o.height),
                "{name}"
            );
            // Те же байты картинки, что в .docx.
            let (image, source) = (p.image.as_ref().unwrap(), o.image.as_ref().unwrap());
            assert_eq!(image.bytes, source.bytes, "{name}");
            assert!(image.name.ends_with(".png"));
            assert_eq!(p.anchor.is_some(), o.anchor.is_some(), "{name}");
        }
    }
    let doc = doc("floating");
    let all = pictures(&doc);
    let first = all[0].anchor.as_ref().unwrap();
    let original = docx("floating");
    let expected = pictures(&original)[0].anchor.clone().unwrap();
    assert_eq!(first.horizontal, expected.horizontal);
    assert_eq!(first.vertical, expected.vertical);
    assert_eq!(first.wrap, expected.wrap);
    assert_eq!(first.distance, expected.distance);
    let behind = all[1].anchor.as_ref().unwrap();
    assert!(behind.behind_text);
    assert_eq!(
        behind.horizontal.placement,
        letter_model::Placement::Align(letter_model::AnchorAlign::Center)
    );
}

#[test]
fn pictures_survive_saving_as_docx() {
    // Картинки из .doc сохраняются в .docx и читаются обратно.
    let doc = doc("floating");
    let bytes = letter_docx::write_bytes(&doc).unwrap();
    let back = letter_docx::read_bytes(&bytes).unwrap();
    let (before, after) = (pictures(&doc), pictures(&back));
    assert_eq!(after.len(), before.len());
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(
            a.image.as_ref().map(|i| &i.bytes),
            b.image.as_ref().map(|i| &i.bytes)
        );
        assert_eq!(a.anchor, b.anchor);
    }
}

#[test]
fn encrypted_documents_open_with_password() {
    // RC4 — файл LibreOffice, RC4 CryptoAPI и XOR — сделаны программой
    // `tests/fixtures/doc/encrypt` из image.doc.
    let plain = doc("image");
    for (name, password) in [
        ("encrypted-rc4", "пароль"),
        ("encrypted-cryptoapi", "пароль"),
        ("encrypted-xor", "secret12"),
    ] {
        let bytes = std::fs::read(dir().join(format!("doc/{name}.doc"))).unwrap();
        assert!(is_doc(&bytes) && is_encrypted(&bytes), "{name}");
        assert!(matches!(read(&bytes), Err(ReadError::Encrypted)), "{name}");
        assert!(
            matches!(
                read_with_password(&bytes, "не тот"),
                Err(ReadError::WrongPassword)
            ),
            "{name}"
        );
        let doc = read_with_password(&bytes, password).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            lines(&doc.plain_text()),
            lines(&plain.plain_text()),
            "{name}"
        );
        // Картинка из потока `Data` тоже расшифрована.
        let (got, expected) = (pictures(&doc), pictures(&plain));
        assert_eq!(got.len(), expected.len(), "{name}");
        for (g, e) in got.iter().zip(&expected) {
            assert_eq!(g.image, e.image, "{name}");
        }
        // Испорченный файл с паролем — ошибка, а не падение.
        for at in (0..bytes.len()).step_by(89) {
            let mut broken = bytes.clone();
            broken[at] ^= 0xA5;
            let _ = read_with_password(&broken, password);
        }
    }
    let plain = std::fs::read(dir().join("doc/image.doc")).unwrap();
    assert!(!is_encrypted(&plain) && !is_encrypted(b"not a doc"));
    // Незашифрованный документ с паролем читается как обычно.
    assert!(read_with_password(&plain, "пароль").is_ok());
}

#[test]
fn russian_text_and_fonts() {
    let doc = doc("ru-en");
    assert!(doc.plain_text().contains("Привет") || doc.plain_text().contains('ё'));
    assert!(doc.styles.defaults.run.fonts.ascii.is_some());
}

#[test]
fn broken_files_are_errors_not_crashes() {
    assert!(matches!(read(b"not a doc"), Err(ReadError::NotADoc)));
    assert!(!is_doc(b"PK\x03\x04"));
    // Обрезанные и испорченные копии: только ошибки, без падений.
    for name in ["table", "floating", "lists"] {
        let bytes = std::fs::read(dir().join(format!("doc/{name}.doc"))).unwrap();
        assert!(is_doc(&bytes));
        for len in (0..bytes.len()).step_by(997) {
            let _ = read(&bytes[..len]);
        }
        for at in (0..bytes.len()).step_by(61) {
            let mut broken = bytes.clone();
            broken[at] ^= 0xA5;
            let _ = read(&broken);
        }
    }
}
