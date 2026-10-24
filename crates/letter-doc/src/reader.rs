//! Сборка модели документа из потоков файла .doc.

use std::io::{Cursor, Read};

use letter_model::{
    Block, Bookmark, BreakKind, CellProps, Columns, Comment, Document, FieldChar, FieldCharKind,
    HalfPoints, HeaderFooter, HeaderFooters, Inline, LineSpacing, Note, NoteKind, NoteReference,
    PageMargins, PageSize, Paragraph, ParagraphProps, Picture, RawXml, Run, RunContent, RunFonts,
    RunProps, Section, SectionProps, Spacing, Style, StyleKind, Table, TableCell, TableRow,
    TableWidth, Twips,
};

use crate::fib::{self, Fib, Stories, Word6, index, plc, u16_at, u32_at};
use crate::pictures::{self, Drawings, Media};
use crate::sprm::{Sep, TableMarks, apply_chp, apply_pap, apply_sep, sprms};
use crate::styles::{ISTD_NIL, StyleSheet, builtin_name, fonts, style_id};
use crate::{ReadError, crypto, links, lists, properties, word6};

/// Самый большой поток, который мы согласны прочитать (512 МБ).
const MAX_STREAM_SIZE: u64 = 512 * 1024 * 1024;

/// Размер страницы оформления (`FKP`) в потоке `WordDocument`.
const PAGE: usize = 512;

/// Номер «стиля по умолчанию» для знаков (`Default Paragraph Font`).
const STI_DEFAULT_FONT: u16 = 65;

/// Открывает составной файл.
pub(crate) fn open(data: &[u8]) -> std::io::Result<cfb::CompoundFile<Cursor<&[u8]>>> {
    cfb::CompoundFile::open(Cursor::new(data))
}

fn stream(file: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, ReadError> {
    let stream = file.open_stream(name).map_err(|_| ReadError::NotADoc)?;
    let mut bytes = Vec::new();
    stream.take(MAX_STREAM_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STREAM_SIZE {
        return Err(ReadError::Corrupt(format!("поток {name} слишком большой")));
    }
    Ok(bytes)
}

/// Кусок текста из таблицы кусков (`Pcd`).
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Позиции знаков (`CP`) куска: от и до.
    cp_start: u32,
    cp_end: u32,
    /// Смещение текста в потоке `WordDocument`.
    fc: u32,
    /// Один байт на знак (Windows-1252), иначе UTF-16.
    compressed: bool,
    /// Номер добавочного оформления (`Prc`), если оно есть.
    prc: Option<usize>,
}

/// Знак основного текста: код UTF-16, смещение в потоке и кусок.
#[derive(Debug, Clone, Copy)]
struct Char {
    code: u16,
    fc: u32,
    piece: usize,
}

/// Оформление знаков или абзацев на отрезке смещений потока.
#[derive(Debug, Clone)]
struct Range {
    start: u32,
    end: u32,
    /// Для абзаца — номер стиля.
    istd: u16,
    grpprl: Vec<u8>,
}

/// Читает документ; `password` нужен, если документ зашифрован.
pub(crate) fn read(data: &[u8], password: Option<&str>) -> Result<Document, ReadError> {
    let mut file = open(data).map_err(|_| ReadError::NotADoc)?;
    let mut word = stream(&mut file, "/WordDocument")?;
    // Где таблицы — видно и в зашифрованном заголовке: первые его байты
    // не шифруются.
    let table_1 = u16_at(&word, 10).is_some_and(|flags| flags & 0x0200 != 0);
    // У Word 6/95 таблицы лежат в самом потоке `WordDocument`.
    let word6 = fib::is_word6(&word);
    let mut table = if word6 {
        Vec::new()
    } else {
        stream(&mut file, if table_1 { "/1Table" } else { "/0Table" })?
    };
    // Картинок, рисунков и сведений может не быть — это не ошибка.
    let mut data = stream(&mut file, "/Data").unwrap_or_default();
    let summary = stream(&mut file, "/\u{5}SummaryInformation").unwrap_or_default();
    if crypto::is_encrypted(&word) {
        let password = password.ok_or(ReadError::Encrypted)?;
        let cipher = crypto::cipher(&word, &table, word6)?;
        let plain = if word6 {
            crypto::PLAIN_HEADER_WORD6
        } else {
            crypto::PLAIN_HEADER
        };
        crypto::decrypt(&cipher, password, plain, &mut word, &mut table, &mut data)?;
    }
    let fib = Fib::read(&word)?;
    if word6 {
        table.clone_from(&word);
    }

    let old = fib.word6.as_ref();
    let font_table = fib.part(&table, index::STTBF_FFN).unwrap_or_default();
    let (fonts, charsets) = match old {
        Some(_) => word6::fonts(font_table)
            .into_iter()
            .map(|f| (f.name, f.charset))
            .unzip(),
        None => (fonts(font_table), Vec::new()),
    };
    let sheet = fib
        .part(&table, index::STSHF)
        .map(|bytes| StyleSheet::read(bytes, old.is_some()))
        .unwrap_or_default();
    let (pieces, prcs) = match (fib.part(&table, index::CLX), old) {
        (Some(clx), _) => read_clx(clx, old.map(|old| old.unicode))?,
        // Без таблицы кусков текст Word 6 идёт одним куском.
        (None, Some(old)) => (
            vec![Piece {
                cp_start: 0,
                cp_end: fib.stories.total(),
                fc: old.fc_min,
                compressed: true,
                prc: None,
            }],
            Vec::new(),
        ),
        (None, None) => return Err(ReadError::Corrupt("нет таблицы кусков текста".into())),
    };
    let chpx = fib
        .part(&table, index::PLCF_BTE_CHPX)
        .map(|plc| read_fkps(&word, plc, false, old))
        .unwrap_or_default();
    let papx = fib
        .part(&table, index::PLCF_BTE_PAPX)
        .map(|plc| read_fkps(&word, plc, true, old))
        .unwrap_or_default();
    let chars = match old {
        None => read_chars(&word, &pieces, fib.stories.total(), &|byte, _| cp1252(byte)),
        Some(old) => {
            let decode = |byte: u8, fc: u32| {
                let font = font_at(fc, &chpx, &papx, &sheet);
                let encoding = charsets
                    .get(usize::from(font))
                    .and_then(|&c| word6::charset_encoding(c))
                    .unwrap_or(old.encoding);
                decode_byte(byte, encoding)
            };
            read_chars(&word, &pieces, fib.stories.total(), &decode)
        }
    };
    let sepxs = fib
        .part(&table, index::PLCF_SED)
        .map(|plc| read_sepxs(&word, plc, old.is_some()))
        .unwrap_or_default();
    let sections = sepxs
        .iter()
        .map(|(cp, grpprl)| {
            let mut sep = Sep::default();
            for (sprm, op) in sprms(grpprl) {
                apply_sep(&mut sep, sprm, op);
            }
            (*cp, sep)
        })
        .collect::<Vec<_>>();

    let notes = Notes::read(&fib, &table);
    let headers = fib
        .part(&table, index::PLCF_HDD)
        .and_then(|bytes| plc(bytes, 0))
        .map(|(positions, _)| positions)
        .unwrap_or_default();
    let dop = fib.part(&table, index::DOP).unwrap_or_default();
    let headers = match old {
        None => headers,
        Some(_) => {
            let flags: Vec<u8> = sepxs
                .iter()
                .map(|(_, grpprl)| {
                    sprms(grpprl)
                        .into_iter()
                        .rev()
                        .find(|(sprm, _)| *sprm == word6::GPRF_IHDT)
                        .and_then(|(_, op)| op.first().copied())
                        .unwrap_or(0)
                })
                .collect();
            word6::expand_headers(&headers, dop.get(1).copied().unwrap_or(0), &flags)
        }
    };

    // «Различать колонтитулы чётных и нечётных страниц» — первый бит `Dop`.
    let even_and_odd = dop.first().is_some_and(|b| b & 1 != 0);

    let drawings = fib
        .part(&table, index::DGG_INFO)
        .map(|content| Drawings::read(content, &word))
        .unwrap_or_default();
    let shapes = |i| -> Vec<(u32, &[u8])> {
        fib.part(&table, i)
            .and_then(|b| plc(b, 26))
            .map(|(cps, spas)| cps.into_iter().zip(spas).collect())
            .unwrap_or_default()
    };
    let (shapes_main, shapes_headers) = (shapes(index::PLC_SPA_MOM), shapes(index::PLC_SPA_HDR));
    let media = Media::default();

    let ids = style_ids(&sheet);
    let mut doc = Document::new();
    fill_styles(&mut doc, &sheet, &ids, &fonts);
    let style_of = |istd: u16| char_style(&ids, istd);
    doc.numbering = lists::read(
        fib.part_and_rest(&table, index::PLF_LST),
        fib.part_and_rest(&table, index::PLF_LFO),
        &lists::Context {
            fonts: &fonts,
            style_id: &style_of,
        },
    );
    let builder = Builder {
        chars: &chars,
        pieces: &pieces,
        prcs: &prcs,
        chpx: &chpx,
        papx: &papx,
        sections: &sections,
        fonts: &fonts,
        ids: &ids,
        stories: fib.stories,
        notes: &notes,
        headers: &headers,
        even_and_odd,
        data: &data,
        drawings: &drawings,
        shapes_main: &shapes_main,
        shapes_headers: &shapes_headers,
        media: &media,
    };
    doc.sections = builder.build();
    doc.footnotes = builder.notes(NoteKind::Footnote);
    doc.endnotes = builder.notes(NoteKind::Endnote);
    doc.comments = builder.comments();
    doc.settings.even_and_odd_headers = even_and_odd;
    doc.properties.core = properties::read(&summary);
    Ok(doc)
}

/// Таблица кусков (`Clx`): добавочное оформление (`Prc`) и куски
/// (`Pcdt`). У Word 6 (`word6` — `Some`) свойства старые, а куски — по
/// байту на знак или, если `Some(true)`, в UTF-16.
fn read_clx(clx: &[u8], word6: Option<bool>) -> Result<(Vec<Piece>, Vec<Vec<u8>>), ReadError> {
    let corrupt = || ReadError::Corrupt("испорчена таблица кусков текста".into());
    let mut prcs = Vec::new();
    let mut at = 0;
    while clx.get(at) == Some(&1) {
        let cb = usize::from(u16_at(clx, at + 1).ok_or_else(corrupt)?);
        let grpprl = clx.get(at + 3..at + 3 + cb).ok_or_else(corrupt)?;
        prcs.push(if word6.is_some() {
            word6::convert(grpprl)
        } else {
            grpprl.to_vec()
        });
        at += 3 + cb;
    }
    if clx.get(at) != Some(&2) {
        return Err(corrupt());
    }
    let lcb = u32_at(clx, at + 1).ok_or_else(corrupt)? as usize;
    let plc_bytes = clx.get(at + 5..at + 5 + lcb).ok_or_else(corrupt)?;
    let (cps, pcds) = plc(plc_bytes, 8).ok_or_else(corrupt)?;
    let mut pieces = Vec::new();
    for (i, pcd) in pcds.iter().enumerate() {
        let fc = u32_at(pcd, 2).ok_or_else(corrupt)?;
        let prm = u16_at(pcd, 6).ok_or_else(corrupt)?;
        let compressed = match word6 {
            Some(unicode) => !unicode,
            None => fc & 0x4000_0000 != 0,
        };
        pieces.push(Piece {
            cp_start: cps[i],
            cp_end: cps[i + 1],
            fc: if word6.is_some() {
                fc
            } else if compressed {
                (fc & !0x4000_0000) / 2
            } else {
                fc
            },
            compressed,
            prc: (prm & 1 == 1).then_some(usize::from(prm >> 1)),
        });
    }
    Ok((pieces, prcs))
}

/// Знаки текста документа (первые `ccp_text`: основной текст, сноски,
/// колонтитулы и т. д.). Номер в списке — позиция знака (`CP`).
/// Однобайтовые знаки переводит `decode` (байт и его смещение).
fn read_chars(
    word: &[u8],
    pieces: &[Piece],
    ccp_text: u32,
    decode: &dyn Fn(u8, u32) -> u16,
) -> Vec<Char> {
    let mut chars = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if piece.cp_start >= ccp_text {
            break;
        }
        let count = piece.cp_end.min(ccp_text).saturating_sub(piece.cp_start);
        for i in 0..count {
            let (code, fc) = if piece.compressed {
                let fc = piece.fc + i;
                let Some(&byte) = word.get(fc as usize) else {
                    break;
                };
                (decode(byte, fc), fc)
            } else {
                let fc = piece.fc + i * 2;
                let Some(code) = u16_at(word, fc as usize) else {
                    break;
                };
                (code, fc)
            };
            chars.push(Char {
                code,
                fc,
                piece: index,
            });
        }
    }
    chars
}

/// Знак Windows-1252 в UTF-16.
fn cp1252(byte: u8) -> u16 {
    decode_byte(byte, encoding_rs::WINDOWS_1252)
}

/// Однобайтовый знак в UTF-16. Служебные знаки (меньше 0x20) — как есть.
fn decode_byte(byte: u8, encoding: &'static encoding_rs::Encoding) -> u16 {
    if byte < 0x20 {
        return u16::from(byte);
    }
    let bytes = [byte];
    let (text, _) = encoding.decode_without_bom_handling(&bytes);
    text.encode_utf16().next().unwrap_or(u16::from(byte))
}

/// Шрифт знака по смещению `fc` (для кодировки текста Word 6): свой,
/// из стиля абзаца (по цепочке «основан на») или шрифт по умолчанию.
fn font_at(fc: u32, chpx: &[Range], papx: &[Range], sheet: &StyleSheet) -> u16 {
    let font = |grpprl: &[u8]| {
        sprms(grpprl)
            .into_iter()
            .rev()
            .find(|(sprm, _)| *sprm == 0x4A4F)
            .and_then(|(_, op)| u16_at(op, 0))
    };
    if let Some(font) = find(chpx, fc).and_then(|r| font(&r.grpprl)) {
        return font;
    }
    let mut istd = find(papx, fc).map_or(0, |r| r.istd);
    for _ in 0..16 {
        let Some(style) = sheet.get(istd) else {
            break;
        };
        if let Some(font) = font(&style.chpx) {
            return font;
        }
        istd = style.based_on;
    }
    sheet.default_fonts[0]
}

/// Страницы оформления (`FKP`) знаков или абзацев по их таблице
/// (`PlcBteChpx`/`PlcBtePapx`). Отрезки упорядочены по смещению.
///
/// У Word 6 (`old`) номер страницы — два байта, запись абзаца — 7 байт,
/// а таблица страниц бывает неполной: недостающие страницы идут подряд.
fn read_fkps(word: &[u8], plc_bytes: &[u8], paragraphs: bool, old: Option<&Word6>) -> Vec<Range> {
    let pn_size = if old.is_some() { 2 } else { 4 };
    let Some((_, pages)) = plc(plc_bytes, pn_size) else {
        return Vec::new();
    };
    let mut pns: Vec<u32> = pages
        .iter()
        .map(|page| match old {
            Some(_) => u32::from(u16_at(page, 0).unwrap_or(0)),
            None => u32_at(page, 0).unwrap_or(0) & 0x003F_FFFF,
        })
        .collect();
    if let Some(old) = old {
        let (first, count) = if paragraphs {
            (old.pn_pap_first, old.cpn_pap)
        } else {
            (old.pn_chp_first, old.cpn_chp)
        };
        let mut next = pns.last().map_or(u32::from(first), |pn| pn + 1);
        while pns.len() < usize::from(count) {
            pns.push(next);
            next += 1;
        }
    }
    let mut ranges = Vec::new();
    for pn in pns {
        let start = pn as usize * PAGE;
        let Some(fkp) = word.get(start..start + PAGE) else {
            continue;
        };
        let count = usize::from(fkp[PAGE - 1]);
        let entry_size = match (paragraphs, old) {
            (false, _) => 1,
            (true, None) => 13,
            (true, Some(_)) => 7,
        };
        let entries = 4 * (count + 1);
        for i in 0..count {
            let (Some(fc_start), Some(fc_end), Some(&offset)) = (
                u32_at(fkp, i * 4),
                u32_at(fkp, (i + 1) * 4),
                fkp.get(entries + i * entry_size),
            ) else {
                break;
            };
            let at = usize::from(offset) * 2;
            let (istd, grpprl) = if offset == 0 {
                (0, Vec::new())
            } else if paragraphs {
                read_papx(fkp, at, old.is_some())
            } else {
                let cb = usize::from(*fkp.get(at).unwrap_or(&0));
                (0, fkp.get(at + 1..at + 1 + cb).unwrap_or_default().to_vec())
            };
            let grpprl = match old {
                Some(_) => word6::convert(&grpprl),
                None => grpprl,
            };
            ranges.push(Range {
                start: fc_start,
                end: fc_end,
                istd,
                grpprl,
            });
        }
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// Оформление абзаца в странице: номер стиля и свойства. В Word 6
/// (`word6`) длина — ровно `2 × cw` байт.
fn read_papx(fkp: &[u8], at: usize, word6: bool) -> (u16, Vec<u8>) {
    let cb = usize::from(*fkp.get(at).unwrap_or(&0));
    let (start, len) = if word6 {
        (at + 1, cb * 2)
    } else if cb == 0 {
        let cb = usize::from(*fkp.get(at + 1).unwrap_or(&0));
        (at + 2, cb * 2)
    } else {
        (at + 1, (cb * 2).saturating_sub(1))
    };
    let Some(data) = fkp.get(start..start + len) else {
        return (0, Vec::new());
    };
    (
        u16_at(data, 0).unwrap_or(0),
        data.get(2..).unwrap_or_default().to_vec(),
    )
}

/// Отрезок, в который попадает смещение `fc`.
fn find(ranges: &[Range], fc: u32) -> Option<&Range> {
    let i = ranges.partition_point(|r| r.start <= fc);
    ranges[..i].last().filter(|r| fc < r.end)
}

/// Разделы: последний знак каждого и его свойства (`Sepx`; у Word 6
/// — переведённые в свойства Word 97).
fn read_sepxs(word: &[u8], plc_bytes: &[u8], word6: bool) -> Vec<(u32, Vec<u8>)> {
    let Some((cps, seds)) = plc(plc_bytes, 12) else {
        return Vec::new();
    };
    seds.iter()
        .enumerate()
        .map(|(i, sed)| {
            let grpprl = u32_at(sed, 2)
                .filter(|&fc| fc != u32::MAX)
                .and_then(|fc| {
                    let cb = u16_at(word, fc as usize)?;
                    let start = fc as usize + 2;
                    word.get(start..start + usize::from(cb))
                })
                .unwrap_or_default();
            let grpprl = if word6 {
                word6::convert(grpprl)
            } else {
                grpprl.to_vec()
            };
            (cps[i + 1], grpprl)
        })
        .collect()
}

/// Идентификаторы стилей по номерам. Повторы получают номер в конце.
fn style_ids(sheet: &StyleSheet) -> Vec<Option<String>> {
    let mut ids: Vec<Option<String>> = Vec::new();
    for entry in &sheet.styles {
        let id = entry.as_ref().filter(|e| e.kind.is_some()).map(|entry| {
            let name = builtin_name(entry.sti).unwrap_or(&entry.name);
            let base = style_id(name);
            let mut id = base.clone();
            let mut n = 1;
            while ids
                .iter()
                .flatten()
                .any(|known| known.eq_ignore_ascii_case(&id))
            {
                n += 1;
                id = format!("{base}{n}");
            }
            id
        });
        ids.push(id);
    }
    ids
}

/// Стиль знаков для фрагмента: «Шрифт абзаца по умолчанию» — это
/// отсутствие стиля.
fn char_style(sheet_ids: &[Option<String>], istd: u16) -> Option<String> {
    sheet_ids.get(usize::from(istd))?.clone()
}

/// Стили и оформление по умолчанию.
fn fill_styles(doc: &mut Document, sheet: &StyleSheet, ids: &[Option<String>], fonts: &[String]) {
    let font = |i: u16| fonts.get(usize::from(i)).cloned();
    let defaults = &mut doc.styles.defaults;
    defaults.run = RunProps {
        fonts: RunFonts {
            ascii: font(sheet.default_fonts[0]),
            east_asia: font(sheet.default_fonts[1]),
            h_ansi: font(sheet.default_fonts[2]),
            complex_script: font(sheet.default_fonts[2]),
            ..RunFonts::default()
        },
        // В .doc размер по умолчанию — 10 пт.
        size: Some(HalfPoints(20)),
        ..RunProps::default()
    };
    defaults.paragraph = ParagraphProps::default();
    doc.styles.styles.clear();

    let style_of = |istd: u16| -> Option<String> {
        let entry = sheet.get(istd)?;
        if entry.sti == STI_DEFAULT_FONT {
            return None;
        }
        char_style(ids, istd)
    };
    for (istd, entry) in sheet.styles.iter().enumerate() {
        let (Some(entry), Some(id)) = (entry, ids.get(istd).cloned().flatten()) else {
            continue;
        };
        let Some(kind) = entry
            .kind
            .filter(|k| matches!(k, StyleKind::Paragraph | StyleKind::Character))
        else {
            continue;
        };
        let mut style = Style::new(id, kind);
        style.name = Some(builtin_name(entry.sti).unwrap_or(&entry.name).to_string());
        style.is_default = (kind == StyleKind::Paragraph && istd == 0)
            || (kind == StyleKind::Character && entry.sti == STI_DEFAULT_FONT);
        style.custom = entry.sti == 0x0FFE;
        let link = |i: u16| {
            (i != ISTD_NIL && usize::from(i) != istd)
                .then(|| char_style(ids, i))
                .flatten()
        };
        style.based_on = link(entry.based_on);
        style.next = link(entry.next);
        let mut marks = TableMarks::default();
        for (sprm, op) in sprms(&entry.papx) {
            apply_pap(&mut style.paragraph, &mut marks, sprm, op);
        }
        drop_plain_level(&mut style.paragraph);
        for (sprm, op) in sprms(&entry.chpx) {
            apply_chp(&mut style.run, sprm, op, fonts, &style_of);
        }
        style.run.style = None;
        doc.styles.styles.push(style);
    }
}

/// Сборщик разделов из знаков и оформления.
struct Builder<'a> {
    chars: &'a [Char],
    pieces: &'a [Piece],
    prcs: &'a [Vec<u8>],
    chpx: &'a [Range],
    papx: &'a [Range],
    sections: &'a [(u32, Sep)],
    fonts: &'a [String],
    ids: &'a [Option<String>],
    stories: Stories,
    notes: &'a Notes,
    /// Границы колонтитулов (`PlcfHdd`) от начала колонтитулов.
    headers: &'a [u32],
    /// Колонтитулы чётных страниц отдельно.
    even_and_odd: bool,
    /// Поток `Data` с картинками в строке.
    data: &'a [u8],
    /// Рисунки документа и плавающие фигуры текста и колонтитулов
    /// (позиция знака привязки и `FSPA`).
    drawings: &'a Drawings,
    shapes_main: &'a [(u32, &'a [u8])],
    shapes_headers: &'a [(u32, &'a [u8])],
    /// Номера файлов картинок.
    media: &'a Media,
}

/// Сноски, концевые сноски, примечания и закладки: где ссылки и где текст.
#[derive(Debug, Clone, Default)]
struct Notes {
    /// Закладки: имя и кусок текста (`CP`).
    bookmarks: Vec<(String, std::ops::Range<u32>)>,
    /// Ссылки на сноски: позиция и «номер ставится сам».
    footnote_refs: Vec<(u32, bool)>,
    /// Границы текста сносок от начала сносок.
    footnote_texts: Vec<u32>,
    endnote_refs: Vec<(u32, bool)>,
    endnote_texts: Vec<u32>,
    /// Ссылки на примечания.
    comment_refs: Vec<u32>,
    /// Границы текста примечаний от начала примечаний.
    comment_texts: Vec<u32>,
    /// Автор и инициалы каждого примечания.
    comment_authors: Vec<(String, String)>,
    /// К какому тексту относится примечание: id и отрезок позиций.
    comments: Vec<(String, std::ops::Range<u32>)>,
}

impl Notes {
    fn read(fib: &Fib, table: &[u8]) -> Notes {
        let part = |i| fib.part(table, i);
        let refs = |i| -> Vec<(u32, bool)> {
            part(i)
                .and_then(|b| plc(b, 2))
                .map(|(cps, frds)| {
                    frds.iter()
                        .zip(cps)
                        .map(|(frd, cp)| (cp, u16_at(frd, 0).unwrap_or(1) != 0))
                        .collect()
                })
                .unwrap_or_default()
        };
        let texts = |i| -> Vec<u32> {
            part(i)
                .and_then(|b| plc(b, 0))
                .map(|(cps, _)| cps)
                .unwrap_or_default()
        };
        let mut notes = Notes {
            bookmarks: bookmarks(fib, table),
            footnote_refs: refs(index::PLCFFND_REF),
            footnote_texts: texts(index::PLCFFND_TXT),
            endnote_refs: refs(index::PLCFEND_REF),
            endnote_texts: texts(index::PLCFEND_TXT),
            comment_texts: texts(index::PLCFAND_TXT),
            ..Notes::default()
        };

        if let Some(old) = &fib.word6 {
            notes.read_word6_comments(fib, table, old);
            return notes;
        }
        // Примечания: ссылки с инициалами, номером автора и меткой
        // закладки, которая отмечает текст примечания.
        let owners = part(index::GRP_XST_ATN_OWNERS)
            .map(xst_list)
            .unwrap_or_default();
        let bookmarks = part(index::STTBF_ATN_BKMK)
            .map(|b| sttb_extra(b).into_iter().map(|e| u32_at(&e, 2)).collect())
            .unwrap_or_else(Vec::new);
        let starts = part(index::PLCF_ATN_BKF).and_then(|b| plc(b, 4));
        let ends = texts(index::PLCF_ATN_BKL);
        if let Some((cps, atrds)) = part(index::PLCFAND_REF).and_then(|b| plc(b, 30)) {
            for (i, (atrd, cp)) in atrds.iter().zip(cps).enumerate() {
                let initials_len = usize::from(u16_at(atrd, 0).unwrap_or(0)).min(9);
                let initials: Vec<u16> = (0..initials_len)
                    .filter_map(|k| u16_at(atrd, 2 + k * 2))
                    .collect();
                let owner = u16_at(atrd, 20).unwrap_or(u16::MAX);
                let author = owners.get(usize::from(owner)).cloned().unwrap_or_default();
                notes
                    .comment_authors
                    .push((author, String::from_utf16_lossy(&initials)));
                notes.comment_refs.push(cp);
                let tag = u32_at(atrd, 26);
                let range = tag
                    .filter(|&t| t != u32::MAX)
                    .and_then(|t| bookmarks.iter().position(|b| *b == Some(t)))
                    .and_then(|j| {
                        let (bkf_cps, fbkfs) = starts.as_ref()?;
                        let start = *bkf_cps.get(j)?;
                        let end = *ends.get(usize::from(u16_at(fbkfs.get(j)?, 0)?))?;
                        Some(start..end)
                    })
                    .unwrap_or(cp..cp);
                notes.comments.push((i.to_string(), range));
            }
        }
        notes
    }
}

impl Notes {
    /// Примечания Word 6: запись ссылки (`ATRD`) — 20 байт: инициалы
    /// однобайтовой строкой и номер автора. Отмеченный текст не читается
    /// (только место примечания).
    fn read_word6_comments(&mut self, fib: &Fib, table: &[u8], old: &Word6) {
        let owners = fib
            .part(table, index::GRP_XST_ATN_OWNERS)
            .map(|b| word6::pascal_list(b, old.encoding))
            .unwrap_or_default();
        let Some((cps, atrds)) = fib.part(table, index::PLCFAND_REF).and_then(|b| plc(b, 20))
        else {
            return;
        };
        for (i, (atrd, cp)) in atrds.iter().zip(cps).enumerate() {
            let initials = word6::pascal_list(&atrd[..10], old.encoding)
                .into_iter()
                .next()
                .unwrap_or_default();
            let owner = u16_at(atrd, 10).unwrap_or(u16::MAX);
            let author = owners.get(usize::from(owner)).cloned().unwrap_or_default();
            self.comment_authors.push((author, initials));
            self.comment_refs.push(cp);
            self.comments.push((i.to_string(), cp..cp));
        }
    }
}

/// Закладки документа: имена из `SttbfBkmk`, начала из `PlcfBkf` (с
/// номером конца) и концы из `PlcfBkl`.
fn bookmarks(fib: &Fib, table: &[u8]) -> Vec<(String, std::ops::Range<u32>)> {
    let names: Vec<(String, Vec<u8>)> = match &fib.word6 {
        Some(old) => fib
            .part(table, index::STTBF_BKMK)
            .map(|b| word6::sttb(b, old.encoding))
            .unwrap_or_default()
            .into_iter()
            .map(|name| (name, Vec::new()))
            .collect(),
        None => fib
            .part(table, index::STTBF_BKMK)
            .map(sttb)
            .unwrap_or_default(),
    };
    let Some((starts, entries)) = fib.part(table, index::PLCF_BKF).and_then(|b| plc(b, 4)) else {
        return Vec::new();
    };
    let ends = fib
        .part(table, index::PLCF_BKL)
        .and_then(|b| plc(b, 0))
        .map(|(cps, _)| cps)
        .unwrap_or_default();
    entries
        .iter()
        .zip(&starts)
        .zip(names)
        .filter_map(|((entry, &start), (name, _))| {
            let end = *ends.get(usize::from(u16_at(entry, 0)?))?;
            Some((name, start..end.max(start)))
        })
        .collect()
}

/// Строки `Xst` подряд: длина в знаках и знаки UTF-16.
fn xst_list(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(len) = u16_at(bytes, at).map(usize::from) {
        let units: Vec<u16> = (0..len)
            .filter_map(|k| u16_at(bytes, at + 2 + k * 2))
            .collect();
        out.push(String::from_utf16_lossy(&units));
        at += 2 + len * 2;
    }
    out
}

/// Строки расширенной таблицы строк (`STTB`, UTF-16) и добавочные
/// данные каждой.
fn sttb(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    if u16_at(bytes, 0) != Some(0xFFFF) {
        return Vec::new();
    }
    let count = usize::from(u16_at(bytes, 2).unwrap_or(0));
    let extra = usize::from(u16_at(bytes, 4).unwrap_or(0));
    let mut at = 6;
    let mut out = Vec::new();
    for _ in 0..count {
        let Some(len) = u16_at(bytes, at).map(usize::from) else {
            break;
        };
        let units: Vec<u16> = (0..len)
            .map_while(|k| u16_at(bytes, at + 2 + k * 2))
            .collect();
        at += 2 + len * 2;
        let data = bytes.get(at..at + extra).unwrap_or_default().to_vec();
        out.push((String::from_utf16_lossy(&units), data));
        at += extra;
    }
    out
}

/// Добавочные данные каждой строки расширенной таблицы строк (`STTB`).
fn sttb_extra(bytes: &[u8]) -> Vec<Vec<u8>> {
    sttb(bytes).into_iter().map(|(_, extra)| extra).collect()
}

/// Таблица, которая сейчас собирается.
#[derive(Default)]
struct TableState {
    rows: Vec<(Vec<TableCell>, TableMarks)>,
    cells: Vec<TableCell>,
    blocks: Vec<Block>,
}

impl TableState {
    fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.cells.is_empty() && self.blocks.is_empty()
    }
}

/// Какую часть текста собираем: от неё зависит смысл особых знаков.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Story {
    /// Основной текст: делится на разделы.
    Main,
    Footnote,
    Endnote,
    Comment,
    Header,
}

impl Builder<'_> {
    /// Разделы основного текста.
    fn build(&self) -> Vec<Section> {
        let parts = self.story(0..self.stories.text, Story::Main);
        parts
            .into_iter()
            .enumerate()
            .map(|(i, blocks)| self.section(i, blocks))
            .collect()
    }

    /// Блоки части текста `range` (позиции `CP`). Основной текст
    /// делится на разделы, остальные части — один список блоков.
    fn story(&self, range: std::ops::Range<u32>, story: Story) -> Vec<Vec<Block>> {
        let mut sections = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut table = TableState::default();
        let mut content: Vec<Inline> = Vec::new();
        let mut run_key = None;
        // Вложенные поля: `true` — сейчас код поля.
        let mut fields: Vec<bool> = Vec::new();
        let start = range.start as usize;
        let end = (range.end as usize).min(self.chars.len());
        let chars = self.chars.get(start..end).unwrap_or_default();

        for (offset, ch) in chars.iter().enumerate() {
            let cp = range.start + offset as u32;
            for (i, (name, range)) in self.notes.bookmarks.iter().enumerate() {
                if range.start == cp {
                    content.push(Inline::BookmarkStart(Bookmark {
                        id: i.to_string(),
                        name: name.clone(),
                    }));
                    run_key = None;
                }
            }
            for (i, (_, range)) in self.notes.bookmarks.iter().enumerate() {
                if range.end == cp {
                    content.push(Inline::BookmarkEnd(i.to_string()));
                    run_key = None;
                }
            }
            if story == Story::Main {
                for (id, _) in self.notes.comments.iter().filter(|(_, r)| r.start == cp) {
                    content.push(Inline::CommentStart(id.clone()));
                    run_key = None;
                }
                for (id, _) in self.notes.comments.iter().filter(|(_, r)| r.end == cp) {
                    content.push(Inline::CommentEnd(id.clone()));
                    run_key = None;
                }
            }
            let key = (find(self.chpx, ch.fc).map(|r| r.start), ch.piece);
            if run_key != Some(key) {
                run_key = Some(key);
                content.push(Inline::Run(Run {
                    props: self.run_props(ch),
                    content: Vec::new(),
                }));
            }
            let in_code = fields.last() == Some(&true);
            let item = match ch.code {
                0x0D | 0x07 => None,
                0x0C if story == Story::Main && self.section_ends_at(cp) => None,
                0x13 => {
                    fields.push(true);
                    Some(RunContent::FieldChar(FieldChar::new(FieldCharKind::Begin)))
                }
                0x14 => {
                    if let Some(last) = fields.last_mut() {
                        *last = false;
                    }
                    Some(RunContent::FieldChar(FieldChar::new(
                        FieldCharKind::Separate,
                    )))
                }
                0x15 => {
                    fields.pop();
                    Some(RunContent::FieldChar(FieldChar::new(FieldCharKind::End)))
                }
                0x0C => Some(RunContent::Break(BreakKind::Page)),
                0x0B => Some(RunContent::Break(BreakKind::Line)),
                0x0E => Some(RunContent::Break(BreakKind::Column)),
                0x09 if !in_code => Some(RunContent::Tab),
                0x1E => Some(text("\u{2011}")),
                0x1F => Some(text("\u{ad}")),
                0x02 => match story {
                    Story::Footnote => Some(RunContent::NoteMark(NoteKind::Footnote)),
                    Story::Endnote => Some(RunContent::NoteMark(NoteKind::Endnote)),
                    _ => self.note_reference(cp).map(RunContent::NoteReference),
                },
                0x05 if story == Story::Main => self
                    .notes
                    .comment_refs
                    .iter()
                    .position(|&c| c == cp)
                    .map(|i| RunContent::CommentReference(i.to_string())),
                // Картинки; знак без картинки (данные поля, фигура) пропускаем.
                0x01 | 0x08 if in_code => continue,
                0x01 | 0x08 => {
                    let picture = if ch.code == 0x01 {
                        self.inline_picture(ch)
                    } else {
                        self.floating_picture(cp, story)
                    };
                    match picture {
                        Some(picture) => Some(RunContent::Picture(picture)),
                        None => continue,
                    }
                }
                // Прочие особые знаки — позже.
                code if code < 0x20 && code != 0x09 => continue,
                code => {
                    let s = String::from_utf16_lossy(&[code]);
                    Some(if in_code {
                        RunContent::FieldCode(s)
                    } else {
                        text(&s)
                    })
                }
            };
            if let Some(item) = item {
                if let Some(Inline::Run(run)) = content.last_mut() {
                    push_content(&mut run.content, item);
                }
                continue;
            }

            // Конец абзаца, ячейки, строки таблицы или раздела.
            let (props, marks) = self.paragraph_props(ch);
            let paragraph = Paragraph {
                props,
                content: take_content(&mut content),
            };
            run_key = None;
            if ch.code == 0x07 && marks.row_end {
                if !table.blocks.is_empty() {
                    let blocks = std::mem::take(&mut table.blocks);
                    table.cells.push(cell(blocks));
                }
                let cells = std::mem::take(&mut table.cells);
                table.rows.push((cells, marks));
            } else if marks.in_table || ch.code == 0x07 {
                table.blocks.push(paragraph.into());
                if ch.code == 0x07 {
                    let blocks = std::mem::take(&mut table.blocks);
                    table.cells.push(cell(blocks));
                }
            } else {
                flush_table(&mut table, &mut blocks);
                blocks.push(paragraph.into());
            }
            if story == Story::Main && ch.code == 0x0C && self.section_ends_at(cp) {
                flush_table(&mut table, &mut blocks);
                sections.push(std::mem::take(&mut blocks));
            }
        }
        let rest = take_content(&mut content);
        if !rest.is_empty() {
            blocks.push(
                Paragraph {
                    props: ParagraphProps::default(),
                    content: rest,
                }
                .into(),
            );
        }
        flush_table(&mut table, &mut blocks);
        if !blocks.is_empty() || sections.is_empty() {
            sections.push(blocks);
        }
        sections
    }

    /// Ссылка на сноску или концевую сноску в позиции `cp`.
    fn note_reference(&self, cp: u32) -> Option<NoteReference> {
        let find = |refs: &[(u32, bool)], kind| {
            refs.iter()
                .position(|&(c, _)| c == cp)
                .map(|i| NoteReference {
                    kind,
                    id: (i + 1).to_string(),
                    custom_mark: !refs[i].1,
                })
        };
        find(&self.notes.footnote_refs, NoteKind::Footnote)
            .or_else(|| find(&self.notes.endnote_refs, NoteKind::Endnote))
    }

    /// Заканчивается ли раздел знаком `cp`.
    fn section_ends_at(&self, cp: u32) -> bool {
        self.sections.iter().any(|(end, _)| *end == cp + 1)
    }

    fn section(&self, index: usize, mut blocks: Vec<Block>) -> Section {
        if blocks.is_empty() {
            blocks.push(Paragraph::default().into());
        }
        let sep = self
            .sections
            .get(index)
            .or(self.sections.last())
            .map(|(_, sep)| *sep)
            .unwrap_or_default();
        let mut props = section_props(&sep);
        // Колонтитулы раздела: после шести служебных частей по шесть на
        // раздел — чётный и нечётный верхние, чётный и нечётный нижние,
        // первой страницы верхний и нижний.
        // Колонтитулы первой и чётных страниц, которые не показываются,
        // не берём: Word хранит их в .doc, но в .docx их нет.
        let story = |k: usize| self.header_story(6 + index * 6 + k);
        let first = |k: usize| story(k).filter(|_| props.title_page);
        let even = |k: usize| story(k).filter(|_| self.even_and_odd);
        props.headers = HeaderFooters {
            default: story(1),
            first: first(4),
            even: even(0),
        };
        props.footers = HeaderFooters {
            default: story(3),
            first: first(5),
            even: even(2),
        };
        Section { props, blocks }
    }

    /// Колонтитул номер `index` в `PlcfHdd`; `None` — пустой (тогда
    /// берётся колонтитул предыдущего раздела).
    fn header_story(&self, index: usize) -> Option<HeaderFooter> {
        let start = *self.headers.get(index)?;
        let end = *self.headers.get(index + 1)?;
        if end <= start {
            return None;
        }
        let base = self.stories.headers_start();
        let mut blocks = self.blocks(base + start..base + end, Story::Header);
        // Каждый колонтитул кончается лишним знаком абзаца.
        if blocks.len() > 1
            && matches!(blocks.last(), Some(Block::Paragraph(p)) if p.content.is_empty())
        {
            blocks.pop();
        }
        Some(HeaderFooter {
            blocks,
            relationship: None,
        })
    }

    /// Блоки части текста одним списком.
    fn blocks(&self, range: std::ops::Range<u32>, story: Story) -> Vec<Block> {
        let mut blocks: Vec<Block> = self.story(range, story).concat();
        if blocks.is_empty() {
            blocks.push(Paragraph::default().into());
        }
        blocks
    }

    /// Сноски или концевые сноски; первыми — служебные черты, как в .docx.
    fn notes(&self, kind: NoteKind) -> Vec<Note> {
        let (refs, texts, start, story) = match kind {
            NoteKind::Footnote => (
                &self.notes.footnote_refs,
                &self.notes.footnote_texts,
                self.stories.footnotes_start(),
                Story::Footnote,
            ),
            NoteKind::Endnote => (
                &self.notes.endnote_refs,
                &self.notes.endnote_texts,
                self.stories.endnotes_start(),
                Story::Endnote,
            ),
        };
        if refs.is_empty() {
            return Vec::new();
        }
        let mut notes = vec![
            separator("-1", "separator"),
            separator("0", "continuationSeparator"),
        ];
        for i in 0..refs.len() {
            let (Some(&a), Some(&b)) = (texts.get(i), texts.get(i + 1)) else {
                break;
            };
            notes.push(Note {
                id: (i + 1).to_string(),
                special: None,
                blocks: self.blocks(start + a..start + b, story),
            });
        }
        notes
    }

    /// Примечания.
    fn comments(&self) -> Vec<Comment> {
        let start = self.stories.comments_start();
        let texts = &self.notes.comment_texts;
        self.notes
            .comment_authors
            .iter()
            .enumerate()
            .map_while(|(i, (author, initials))| {
                let (&a, &b) = (texts.get(i)?, texts.get(i + 1)?);
                Some(Comment {
                    id: i.to_string(),
                    author: author.clone(),
                    initials: (!initials.is_empty()).then(|| initials.clone()),
                    blocks: self.blocks(start + a..start + b, Story::Comment),
                    ..Comment::default()
                })
            })
            .collect()
    }

    /// Добавочное оформление куска (после «быстрого сохранения»).
    /// Картинка в строке: знак 0x01 со ссылкой в поток `Data`
    /// (`sprmCPicLocation`).
    fn inline_picture(&self, ch: &Char) -> Option<Picture> {
        let direct = find(self.chpx, ch.fc).map_or(&[][..], |r| r.grpprl.as_slice());
        let all: Vec<_> = [direct, self.prc(ch)].into_iter().flat_map(sprms).collect();
        // Данные поля (`sprmCFData`) — не картинка.
        if all
            .iter()
            .any(|(sprm, op)| *sprm == 0x0806 && op.first() != Some(&0))
        {
            return None;
        }
        let offset = all
            .iter()
            .filter(|(sprm, _)| *sprm == 0x6A03)
            .filter_map(|(_, op)| u32_at(op, 0))
            .next_back()?;
        pictures::inline(self.data, offset as usize, self.media)
    }

    /// Плавающая картинка у знака привязки 0x08 в позиции `cp`.
    fn floating_picture(&self, cp: u32, story: Story) -> Option<Picture> {
        let (shapes, cp) = match story {
            Story::Main => (self.shapes_main, cp),
            Story::Header => (
                self.shapes_headers,
                cp.checked_sub(self.stories.headers_start())?,
            ),
            _ => return None,
        };
        let (_, fspa) = shapes.iter().find(|(at, _)| *at == cp)?;
        self.drawings.floating(fspa, self.media)
    }

    fn prc(&self, ch: &Char) -> &[u8] {
        self.pieces
            .get(ch.piece)
            .and_then(|p| p.prc)
            .and_then(|i| self.prcs.get(i))
            .map_or(&[], Vec::as_slice)
    }

    fn run_props(&self, ch: &Char) -> RunProps {
        let mut props = RunProps::default();
        let ids = self.ids;
        let style_of = |istd: u16| char_style(ids, istd);
        let direct = find(self.chpx, ch.fc).map_or(&[][..], |r| r.grpprl.as_slice());
        for grpprl in [direct, self.prc(ch)] {
            for (sprm, op) in sprms(grpprl) {
                apply_chp(&mut props, sprm, op, self.fonts, &style_of);
            }
        }
        // «Шрифт абзаца по умолчанию» — то же, что без стиля.
        if props.style.as_deref() == Some("DefaultParagraphFont") {
            props.style = None;
        }
        props
    }

    fn paragraph_props(&self, ch: &Char) -> (ParagraphProps, TableMarks) {
        let mut props = ParagraphProps::default();
        let mut marks = TableMarks::default();
        let range = find(self.papx, ch.fc);
        if let Some(range) = range {
            if range.istd != 0 {
                props.style = char_style(self.ids, range.istd);
            }
            for (sprm, op) in sprms(&range.grpprl) {
                apply_pap(&mut props, &mut marks, sprm, op);
            }
        }
        for (sprm, op) in sprms(self.prc(ch)) {
            apply_pap(&mut props, &mut marks, sprm, op);
        }
        drop_plain_level(&mut props);
        (props, marks)
    }
}

/// Word пишет «уровень 0» и абзацам не из списка: такая ссылка на
/// список ничего не значит, убираем её.
fn drop_plain_level(props: &mut ParagraphProps) {
    if props
        .numbering
        .is_some_and(|n| n.id.is_none() && n.level.unwrap_or(0) == 0)
    {
        props.numbering = None;
    }
}

/// Содержимое абзаца без пустых фрагментов.
fn take_content(content: &mut Vec<Inline>) -> Vec<Inline> {
    links::hyperlinks(
        std::mem::take(content)
            .into_iter()
            .filter(|i| !matches!(i, Inline::Run(r) if r.content.is_empty()))
            .collect(),
    )
}

fn text(s: &str) -> RunContent {
    RunContent::Text(s.to_string())
}

/// Добавляет содержимое во фрагмент, склеивая текст с текстом.
fn push_content(content: &mut Vec<RunContent>, item: RunContent) {
    match (content.last_mut(), item) {
        (Some(RunContent::Text(last)), RunContent::Text(more)) => last.push_str(&more),
        (Some(RunContent::FieldCode(last)), RunContent::FieldCode(more)) => last.push_str(&more),
        (_, item) => content.push(item),
    }
}

/// Служебная сноска-черта (`separator`, `continuationSeparator`).
fn separator(id: &str, kind: &str) -> Note {
    let mut paragraph = Paragraph::from_runs([Run {
        props: RunProps::default(),
        content: vec![RunContent::Unknown(RawXml::new(format!("<w:{kind}/>")))],
    }]);
    paragraph.props.spacing = Spacing {
        after: Some(Twips(0)),
        line: Some(LineSpacing::Multiple(240)),
        ..Spacing::default()
    };
    Note {
        id: id.to_string(),
        special: Some(kind.to_string()),
        blocks: vec![Block::Paragraph(paragraph)],
    }
}

fn cell(mut blocks: Vec<Block>) -> TableCell {
    if blocks.is_empty() {
        blocks.push(Paragraph::default().into());
    }
    TableCell {
        props: CellProps::default(),
        raw_props: Vec::new(),
        blocks,
    }
}

/// Дописывает собранную таблицу в блоки. Сетка столбцов — все границы
/// ячеек всех строк; ячейка занимает столбцы между своими границами.
fn flush_table(state: &mut TableState, blocks: &mut Vec<Block>) {
    if state.is_empty() {
        return;
    }
    if !state.blocks.is_empty() {
        let cell_blocks = std::mem::take(&mut state.blocks);
        state.cells.push(cell(cell_blocks));
    }
    if !state.cells.is_empty() {
        let cells = std::mem::take(&mut state.cells);
        state.rows.push((cells, TableMarks::default()));
    }
    let mut edges: Vec<i16> = state
        .rows
        .iter()
        .flat_map(|(_, marks)| marks.cell_edges.iter().copied())
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut table = Table {
        grid: edges
            .windows(2)
            .map(|w| Twips(i32::from(w[1]) - i32::from(w[0])))
            .collect(),
        ..Table::default()
    };
    table.props.borders = state
        .rows
        .iter()
        .find_map(|(_, marks)| marks.table_borders.clone());
    for (cells, marks) in std::mem::take(&mut state.rows) {
        let mut cells = cells;
        let row_edges = &marks.cell_edges;
        for (i, cell) in cells.iter_mut().enumerate() {
            if let Some(look) = marks.cells.get(i) {
                if !look.borders.is_empty() {
                    cell.props.borders = Some(look.borders.clone());
                }
                cell.props.shading = look.shading.clone().or_else(|| look.shading80.clone());
            }
            let (Some(&left), Some(&right)) = (row_edges.get(i), row_edges.get(i + 1)) else {
                continue;
            };
            cell.props.width = Some(TableWidth::Twips(Twips(i32::from(right) - i32::from(left))));
            let span = edges.iter().filter(|&&e| e > left && e <= right).count();
            if span > 1 {
                cell.props.grid_span = Some(span as u32);
            }
        }
        table.rows.push(TableRow {
            props: marks.row,
            raw_props: Vec::new(),
            cells,
        });
    }
    blocks.push(table.into());
}

fn section_props(sep: &Sep) -> SectionProps {
    SectionProps {
        page_size: Some(PageSize {
            width: Twips(sep.width),
            height: Twips(sep.height),
            orientation: sep.orientation(),
        }),
        margins: Some(PageMargins {
            top: Twips(sep.top),
            right: Twips(sep.right),
            bottom: Twips(sep.bottom),
            left: Twips(sep.left),
            header: Twips(sep.header),
            footer: Twips(sep.footer),
            gutter: Twips(sep.gutter),
        }),
        start: Some(sep.start),
        columns: Some(Columns {
            count: sep.columns,
            space: Twips(sep.column_space),
            separator: sep.column_line,
            widths: Vec::new(),
        }),
        title_page: sep.title_page,
        ..SectionProps::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_found_by_offset() {
        let range = |start, end| Range {
            start,
            end,
            istd: 0,
            grpprl: Vec::new(),
        };
        let ranges = [range(10, 20), range(20, 30), range(40, 50)];
        assert_eq!(find(&ranges, 10).map(|r| r.start), Some(10));
        assert_eq!(find(&ranges, 29).map(|r| r.start), Some(20));
        assert!(find(&ranges, 35).is_none());
        assert!(find(&ranges, 5).is_none());
    }

    #[test]
    fn windows_1252_letters() {
        assert_eq!(cp1252(b'A'), u16::from(b'A'));
        assert_eq!(cp1252(0x93), 0x201C);
        assert_eq!(cp1252(0xE9), 0xE9);
    }
}
