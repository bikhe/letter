//! Правка текста: ввод букв, табуляции и разрывов, Enter, Backspace,
//! Delete.
//!
//! Правки меняют модель документа ([`Document`]), после них документ
//! верстается заново. Позиции — как у курсора ([`Position`]): номер
//! абзаца по порядку во всём документе и смещение в байтах в тексте
//! абзаца ([`Paragraph::plain_text`]). В этом тексте табуляция и разрыв —
//! по одному знаку, а непонятые элементы (рисунки, поля) — ноль знаков.

use letter_model::{Block, BreakKind, Document, Inline, Paragraph, Run, RunContent};
use unicode_segmentation::UnicodeSegmentation;

use crate::position::Position;

/// Что вставить в текст.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert<'a> {
    /// Буквы (без табуляций и переводов строк).
    Text(&'a str),
    /// Табуляция (Tab).
    Tab,
    /// Разрыв строки (Shift+Enter).
    LineBreak,
    /// Разрыв страницы (Ctrl+Enter).
    PageBreak,
}

/// Вставляет `what` в позицию `at`. Новые буквы получают оформление
/// буквы перед курсором (в начале абзаца — первой буквы), как в Word.
/// Возвращает позицию сразу после вставленного; `None`, если такого
/// абзаца нет или смещение не на границе букв.
pub fn insert(doc: &mut Document, at: Position, what: Insert) -> Option<Position> {
    let paragraph = paragraph_mut(doc, at.paragraph)?;
    if !paragraph.plain_text().is_char_boundary(at.offset) {
        return None;
    }
    let item = match what {
        Insert::Text("") => return Some(at),
        Insert::Text(text) => RunContent::Text(text.to_owned()),
        Insert::Tab => RunContent::Tab,
        Insert::LineBreak => RunContent::Break(BreakKind::Line),
        Insert::PageBreak => RunContent::Break(BreakKind::Page),
    };
    let added = content_len(&item);
    let (run, index) = split_at(paragraph, at.offset)?;
    let content = &mut run.content;
    match (&item, index.checked_sub(1).map(|i| &mut content[i])) {
        (RunContent::Text(text), Some(RunContent::Text(before))) => before.push_str(text),
        _ => content.insert(index, item),
    }
    merge_texts(content);
    Some(Position::new(at.paragraph, at.offset + added))
}

/// Enter: делит абзац в позиции `at` на два. Новый абзац получает то же
/// оформление абзаца, текст после курсора и оформление букв на месте
/// разделения (чтобы печать продолжалась тем же шрифтом). Возвращает
/// начало нового абзаца.
pub fn split_paragraph(doc: &mut Document, at: Position) -> Option<Position> {
    let place = locate(doc, at.paragraph)?;
    let blocks = blocks_mut(doc, &place)?;
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(place.block) else {
        return None;
    };
    if !paragraph.plain_text().is_char_boundary(at.offset) {
        return None;
    }
    let mut second = Paragraph {
        props: paragraph.props.clone(),
        content: Vec::new(),
    };
    if let Some((inline, index)) = split_point(paragraph, at.offset) {
        let mut rest = paragraph.content.split_off(inline + 1);
        if let Some(Inline::Run(run)) = paragraph.content.get_mut(inline) {
            let tail = run.content.split_off(index);
            rest.insert(
                0,
                Inline::Run(Run {
                    props: run.props.clone(),
                    content: tail,
                }),
            );
        }
        second.content = rest;
    }
    tidy(paragraph);
    tidy(&mut second);
    blocks.insert(place.block + 1, Block::Paragraph(second));
    Some(Position::new(at.paragraph + 1, 0))
}

/// Backspace: удаляет букву перед курсором, а в начале абзаца — соединяет
/// абзац с предыдущим. Возвращает новую позицию курсора; `None`, если
/// удалять нечего (начало документа, начало ячейки таблицы).
pub fn delete_backward(doc: &mut Document, at: Position) -> Option<Position> {
    let text = paragraph_mut(doc, at.paragraph)?.plain_text();
    if at.offset == 0 {
        let previous = at.paragraph.checked_sub(1)?;
        let length = paragraph_mut(doc, previous)?.plain_text().len();
        join_with_next(doc, previous)?;
        return Some(Position::new(previous, length));
    }
    let grapheme = text.get(..at.offset)?.graphemes(true).next_back()?;
    let start = at.offset - grapheme.len();
    delete_range(paragraph_mut(doc, at.paragraph)?, start, at.offset);
    Some(Position::new(at.paragraph, start))
}

/// Delete: удаляет букву после курсора, а в конце абзаца — присоединяет
/// следующий абзац. Курсор остаётся на месте; `None`, если удалять
/// нечего.
pub fn delete_forward(doc: &mut Document, at: Position) -> Option<Position> {
    let text = paragraph_mut(doc, at.paragraph)?.plain_text();
    if at.offset >= text.len() {
        join_with_next(doc, at.paragraph)?;
        return Some(Position::new(at.paragraph, text.len()));
    }
    let grapheme = text.get(at.offset..)?.graphemes(true).next()?;
    let end = at.offset + grapheme.len();
    delete_range(paragraph_mut(doc, at.paragraph)?, at.offset, end);
    Some(Position::new(at.paragraph, at.offset))
}

/// Сколько всего абзацев в документе (вместе с абзацами в таблицах).
pub fn paragraph_count(doc: &Document) -> usize {
    fn count(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(_) => 1,
                Block::Table(table) => table
                    .rows
                    .iter()
                    .flat_map(|r| &r.cells)
                    .map(|c| count(&c.blocks))
                    .sum(),
                Block::Unknown(_) => 0,
            })
            .sum()
    }
    doc.sections.iter().map(|s| count(&s.blocks)).sum()
}

/// Абзац с номером `index` (по порядку, как в вёрстке).
pub fn paragraph_mut(doc: &mut Document, index: usize) -> Option<&mut Paragraph> {
    let place = locate(doc, index)?;
    match blocks_mut(doc, &place)?.get_mut(place.block) {
        Some(Block::Paragraph(p)) => Some(p),
        _ => None,
    }
}

/// Где лежит абзац: раздел, путь через ячейки таблиц (номер блока
/// таблицы, строки, ячейки) и номер блока в последнем списке.
struct Place {
    section: usize,
    cells: Vec<(usize, usize, usize)>,
    block: usize,
}

fn locate(doc: &Document, index: usize) -> Option<Place> {
    fn walk(
        blocks: &[Block],
        index: usize,
        seen: &mut usize,
        cells: &mut Vec<(usize, usize, usize)>,
    ) -> Option<usize> {
        for (b, block) in blocks.iter().enumerate() {
            match block {
                Block::Paragraph(_) => {
                    if *seen == index {
                        return Some(b);
                    }
                    *seen += 1;
                }
                Block::Table(table) => {
                    for (r, row) in table.rows.iter().enumerate() {
                        for (c, cell) in row.cells.iter().enumerate() {
                            cells.push((b, r, c));
                            if let Some(found) = walk(&cell.blocks, index, seen, cells) {
                                return Some(found);
                            }
                            cells.pop();
                        }
                    }
                }
                Block::Unknown(_) => {}
            }
        }
        None
    }
    let mut seen = 0;
    for (s, section) in doc.sections.iter().enumerate() {
        let mut cells = Vec::new();
        if let Some(block) = walk(&section.blocks, index, &mut seen, &mut cells) {
            return Some(Place {
                section: s,
                cells,
                block,
            });
        }
    }
    None
}

fn blocks_mut<'a>(doc: &'a mut Document, place: &Place) -> Option<&'a mut Vec<Block>> {
    let mut blocks = &mut doc.sections.get_mut(place.section)?.blocks;
    for &(b, r, c) in &place.cells {
        let Some(Block::Table(table)) = blocks.get_mut(b) else {
            return None;
        };
        blocks = &mut table.rows.get_mut(r)?.cells.get_mut(c)?.blocks;
    }
    Some(blocks)
}

/// Присоединяет к абзацу `index` следующий, если он стоит сразу за ним
/// в том же месте (не в другой ячейке и не после таблицы). Оформление
/// абзаца — первого, а если первый пуст — второго.
fn join_with_next(doc: &mut Document, index: usize) -> Option<()> {
    let place = locate(doc, index)?;
    let next = locate(doc, index + 1)?;
    if next.section != place.section || next.cells != place.cells || next.block != place.block + 1 {
        return None;
    }
    let blocks = blocks_mut(doc, &place)?;
    let Some(Block::Paragraph(second)) = blocks.get(next.block).cloned() else {
        return None;
    };
    let Some(Block::Paragraph(first)) = blocks.get_mut(place.block) else {
        return None;
    };
    if first.plain_text().is_empty() {
        first.props = second.props;
    }
    first.content.extend(second.content);
    tidy(first);
    blocks.remove(next.block);
    Some(())
}

/// Длина элемента в тексте абзаца, байты.
fn content_len(item: &RunContent) -> usize {
    match item {
        RunContent::Text(t) => t.len(),
        RunContent::Tab | RunContent::Break(_) => 1,
        RunContent::Unknown(_) => 0,
    }
}

fn run_len(run: &Run) -> usize {
    run.content.iter().map(content_len).sum()
}

/// Где в абзаце место `offset`: номер фрагмента в [`Paragraph::content`]
/// и номер элемента в нём, перед которым это место (текст на месте
/// разделяется на два элемента). Берётся фрагмент, в котором буква перед
/// местом; в начале абзаца — первый фрагмент. `None`, если фрагментов
/// нет.
fn split_point(paragraph: &mut Paragraph, offset: usize) -> Option<(usize, usize)> {
    let mut start = 0;
    let mut chosen = None;
    for (i, inline) in paragraph.content.iter().enumerate() {
        let Inline::Run(run) = inline else { continue };
        let end = start + run_len(run);
        if chosen.is_none() || start < offset && offset <= end {
            chosen = Some((i, start));
        }
        if start < offset && offset <= end {
            break;
        }
        start = end;
    }
    let (inline, run_start) = chosen?;
    let Some(Inline::Run(run)) = paragraph.content.get_mut(inline) else {
        return None;
    };
    let local = offset.checked_sub(run_start)?;
    let mut at = 0;
    for i in 0..run.content.len() {
        if at == local {
            return Some((inline, i));
        }
        let length = content_len(&run.content[i]);
        if local < at + length {
            if let RunContent::Text(text) = &mut run.content[i] {
                let tail = text.split_off(local - at);
                run.content.insert(i + 1, RunContent::Text(tail));
                return Some((inline, i + 1));
            }
            return None;
        }
        at += length;
    }
    Some((inline, run.content.len()))
}

/// Фрагмент и номер элемента в нём для вставки в место `offset`. Если
/// фрагментов нет, добавляет пустой.
fn split_at(paragraph: &mut Paragraph, offset: usize) -> Option<(&mut Run, usize)> {
    if !paragraph
        .content
        .iter()
        .any(|i| matches!(i, Inline::Run(_)))
    {
        paragraph.content.push(Inline::Run(Run::default()));
    }
    let (inline, index) = split_point(paragraph, offset)?;
    match paragraph.content.get_mut(inline) {
        Some(Inline::Run(run)) => Some((run, index)),
        _ => None,
    }
}

/// Удаляет из абзаца текст `start..end` (байты, на границах знаков).
fn delete_range(paragraph: &mut Paragraph, start: usize, end: usize) {
    let mut at = 0;
    for inline in &mut paragraph.content {
        let Inline::Run(run) = inline else { continue };
        run.content.retain_mut(|item| {
            let length = content_len(item);
            let (s, e) = (at, at + length);
            at = e;
            let (from, to) = (start.max(s), end.min(e));
            if from >= to {
                return true;
            }
            match item {
                RunContent::Text(text) => {
                    text.replace_range(from - s..to - s, "");
                    !text.is_empty()
                }
                _ => false,
            }
        });
        merge_texts(&mut run.content);
    }
    tidy(paragraph);
}

/// Склеивает соседние куски текста в одном фрагменте.
fn merge_texts(content: &mut Vec<RunContent>) {
    let mut i = 1;
    while i < content.len() {
        if let (RunContent::Text(_), RunContent::Text(_)) = (&content[i - 1], &content[i]) {
            if let RunContent::Text(next) = content.remove(i)
                && let RunContent::Text(previous) = &mut content[i - 1]
            {
                previous.push_str(&next);
            }
        } else {
            i += 1;
        }
    }
}

/// Убирает пустые фрагменты, но последний оставляет: в нём оформление,
/// которым продолжится печать.
fn tidy(paragraph: &mut Paragraph) {
    let runs = paragraph
        .content
        .iter()
        .filter(|i| matches!(i, Inline::Run(_)))
        .count();
    let empty = |i: &Inline| matches!(i, Inline::Run(r) if r.content.is_empty());
    let empties = paragraph.content.iter().filter(|i| empty(i)).count();
    if empties == 0 {
        return;
    }
    let mut keep_one = empties == runs;
    paragraph.content.retain(|i| {
        if !empty(i) {
            return true;
        }
        std::mem::replace(&mut keep_one, false)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{RunProps, Table, TableCell, TableRow};

    fn doc(paragraphs: &[&str]) -> Document {
        let mut doc = Document::new();
        doc.sections[0].blocks = paragraphs
            .iter()
            .map(|t| Block::Paragraph(Paragraph::text(*t)))
            .collect();
        doc
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.plain_text().split('\n').map(str::to_owned).collect()
    }

    fn bold() -> RunProps {
        RunProps {
            bold: Some(true),
            ..RunProps::default()
        }
    }

    #[test]
    fn typing_inserts_text() {
        let mut d = doc(&["Hllo"]);
        let p = insert(&mut d, Position::new(0, 1), Insert::Text("e")).unwrap();
        assert_eq!(p, Position::new(0, 2));
        let p = insert(&mut d, Position::new(0, 5), Insert::Text(", мир")).unwrap();
        assert_eq!(p, Position::new(0, 5 + ", мир".len()));
        assert_eq!(texts(&d), ["Hello, мир"]);
        // Один фрагмент, один кусок текста.
        let Block::Paragraph(p) = &d.sections[0].blocks[0] else {
            panic!()
        };
        assert_eq!(p.content.len(), 1);
        // Не на границе буквы — отказ.
        assert_eq!(insert(&mut d, Position::new(0, 8), Insert::Text("x")), None);
        assert_eq!(insert(&mut d, Position::new(5, 0), Insert::Text("x")), None);
    }

    #[test]
    fn typing_takes_formatting_of_the_letter_before() {
        let mut d = Document::new();
        d.sections[0].blocks = vec![Block::Paragraph(Paragraph::from_runs([
            Run::text("ab").with_props(bold()),
            Run::text("cd"),
        ]))];
        insert(&mut d, Position::new(0, 2), Insert::Text("X")).unwrap();
        insert(&mut d, Position::new(0, 0), Insert::Text("Y")).unwrap();
        let p = paragraph_mut(&mut d, 0).unwrap();
        let runs: Vec<_> = p.runs().map(|r| (r.plain_text(), r.props.bold)).collect();
        assert_eq!(
            runs,
            [("YabX".to_owned(), Some(true)), ("cd".to_owned(), None)]
        );
    }

    #[test]
    fn tabs_and_breaks_are_one_character() {
        let mut d = doc(&["ab"]);
        let p = insert(&mut d, Position::new(0, 1), Insert::Tab).unwrap();
        let p = insert(&mut d, p, Insert::LineBreak).unwrap();
        assert_eq!(p, Position::new(0, 3));
        insert(&mut d, Position::new(0, 4), Insert::PageBreak).unwrap();
        let text = paragraph_mut(&mut d, 0).unwrap().plain_text();
        assert_eq!(text, "a\t\nb\u{c}");
        // Backspace удаляет разрыв целиком.
        let p = delete_backward(&mut d, Position::new(0, 3)).unwrap();
        assert_eq!(p, Position::new(0, 2));
        assert_eq!(paragraph_mut(&mut d, 0).unwrap().plain_text(), "a\tb\u{c}");
    }

    #[test]
    fn enter_splits_and_backspace_joins() {
        let mut d = Document::new();
        d.sections[0].blocks = vec![Block::Paragraph(Paragraph::from_runs([
            Run::text("Hello ").with_props(bold()),
            Run::text("world"),
        ]))];
        let p = split_paragraph(&mut d, Position::new(0, 3)).unwrap();
        assert_eq!(p, Position::new(1, 0));
        assert_eq!(texts(&d), ["Hel", "lo world"]);
        // Начало второго абзаца — жирное, как место разделения.
        let second = paragraph_mut(&mut d, 1).unwrap();
        assert_eq!(second.runs().next().unwrap().props.bold, Some(true));

        let p = delete_backward(&mut d, Position::new(1, 0)).unwrap();
        assert_eq!(p, Position::new(0, 3));
        assert_eq!(texts(&d), ["Hello world"]);

        // Enter в конце — пустой абзац с тем же оформлением букв.
        let p = split_paragraph(&mut d, Position::new(0, 11)).unwrap();
        assert_eq!(texts(&d), ["Hello world", ""]);
        let p = insert(&mut d, p, Insert::Text("!")).unwrap();
        assert_eq!(p, Position::new(1, 1));
        assert_eq!(texts(&d), ["Hello world", "!"]);
        // Enter в начале — пустой абзац перед текстом.
        split_paragraph(&mut d, Position::new(0, 0)).unwrap();
        assert_eq!(texts(&d), ["", "Hello world", "!"]);
    }

    #[test]
    fn delete_removes_graphemes_and_joins_forward() {
        let mut d = doc(&["ай\u{306}", "b"]);
        // «й» из двух кодов удаляется целиком.
        let p = delete_backward(&mut d, Position::new(0, 6)).unwrap();
        assert_eq!(p, Position::new(0, 2));
        assert_eq!(texts(&d), ["а", "b"]);
        let p = delete_forward(&mut d, Position::new(0, 2)).unwrap();
        assert_eq!(p, Position::new(0, 2));
        assert_eq!(texts(&d), ["аb"]);
        let p = delete_forward(&mut d, Position::new(0, 0)).unwrap();
        assert_eq!(p, Position::new(0, 0));
        assert_eq!(texts(&d), ["b"]);
        // Края документа.
        assert_eq!(delete_backward(&mut d, Position::new(0, 0)), None);
        assert_eq!(delete_forward(&mut d, Position::new(0, 1)), None);
    }

    #[test]
    fn deleting_everything_keeps_the_formatting() {
        let mut d = Document::new();
        d.sections[0].blocks = vec![Block::Paragraph(Paragraph::from_runs([
            Run::text("x").with_props(bold())
        ]))];
        delete_backward(&mut d, Position::new(0, 1)).unwrap();
        insert(&mut d, Position::new(0, 0), Insert::Text("y")).unwrap();
        let p = paragraph_mut(&mut d, 0).unwrap();
        assert_eq!(p.runs().count(), 1);
        assert_eq!(p.runs().next().unwrap().props.bold, Some(true));
    }

    #[test]
    fn paragraphs_in_tables() {
        let mut d = doc(&["before", "after"]);
        let table = Table {
            rows: vec![TableRow {
                cells: vec![TableCell::text("A1"), TableCell::text("B1")],
                ..TableRow::default()
            }],
            ..Table::default()
        };
        d.sections[0].blocks.insert(1, Block::Table(table));
        assert_eq!(paragraph_count(&d), 4);
        // Абзац 2 — ячейка B1.
        insert(&mut d, Position::new(2, 2), Insert::Text("!")).unwrap();
        assert_eq!(paragraph_mut(&mut d, 2).unwrap().plain_text(), "B1!");
        // Enter в ячейке — новый абзац в ней же.
        split_paragraph(&mut d, Position::new(1, 1)).unwrap();
        assert_eq!(paragraph_count(&d), 5);
        assert_eq!(paragraph_mut(&mut d, 2).unwrap().plain_text(), "1");
        // Backspace в начале ячейки и после таблицы ничего не соединяет.
        assert_eq!(delete_backward(&mut d, Position::new(3, 0)), None);
        assert_eq!(delete_backward(&mut d, Position::new(4, 0)), None);
        assert_eq!(delete_backward(&mut d, Position::new(1, 0)), None);
        assert_eq!(paragraph_count(&d), 5);
        // А внутри ячейки соединяет.
        assert_eq!(
            delete_backward(&mut d, Position::new(2, 0)),
            Some(Position::new(1, 1))
        );
        assert_eq!(paragraph_mut(&mut d, 1).unwrap().plain_text(), "A1");
    }
}
