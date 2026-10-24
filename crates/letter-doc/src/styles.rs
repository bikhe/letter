//! Таблица стилей (`STSH`, MS-DOC 2.9.271) и шрифтов (`SttbfFfn`).

use letter_model::StyleKind;

use crate::fib::u16_at;

/// Стиль из таблицы стилей.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StyleEntry {
    /// Номер встроенного стиля (`sti`); 0x0FFE — стиль пользователя.
    pub sti: u16,
    /// Абзаца, знаков или другой.
    pub kind: Option<StyleKind>,
    /// Имя.
    pub name: String,
    /// На каком стиле основан (`istd`); 0x0FFF — ни на каком.
    pub based_on: u16,
    /// Стиль следующего абзаца.
    pub next: u16,
    /// Оформление абзаца (без номера стиля впереди).
    pub papx: Vec<u8>,
    /// Оформление знаков.
    pub chpx: Vec<u8>,
}

/// Таблица стилей: стили по номерам (`istd`) и шрифты по умолчанию.
#[derive(Debug, Clone, Default)]
pub(crate) struct StyleSheet {
    /// Стили; пустые места — `None`.
    pub styles: Vec<Option<StyleEntry>>,
    /// Номера шрифтов по умолчанию: ASCII, восточноазиатский, прочий.
    pub default_fonts: [u16; 3],
}

/// «Ни на каком стиле».
pub(crate) const ISTD_NIL: u16 = 0x0FFF;

impl StyleSheet {
    /// Разбирает таблицу стилей. Испорченные стили пропускаются.
    /// У Word 6 (`word6`) имена однобайтовые, один шрифт по умолчанию,
    /// а свойства переводятся в свойства Word 97.
    pub fn read(bytes: &[u8], word6: bool) -> StyleSheet {
        let mut sheet = StyleSheet::default();
        let Some(cb_stshi) = u16_at(bytes, 0).map(usize::from) else {
            return sheet;
        };
        let stshi = bytes.get(2..2 + cb_stshi).unwrap_or_default();
        let count = u16_at(stshi, 0).unwrap_or(0);
        let base_size = usize::from(u16_at(stshi, 2).unwrap_or(10));
        for (i, slot) in sheet.default_fonts.iter_mut().enumerate() {
            *slot = u16_at(stshi, if word6 { 12 } else { 12 + i * 2 }).unwrap_or(0);
        }
        let mut at = 2 + cb_stshi;
        for _ in 0..count {
            let Some(cb) = u16_at(bytes, at).map(usize::from) else {
                break;
            };
            at += 2;
            let std = bytes.get(at..at + cb);
            at += cb;
            sheet.styles.push(
                std.filter(|s| !s.is_empty())
                    .and_then(|s| read_std(s, base_size, word6)),
            );
        }
        sheet
    }

    /// Стиль по номеру.
    pub fn get(&self, istd: u16) -> Option<&StyleEntry> {
        self.styles.get(usize::from(istd))?.as_ref()
    }
}

/// Один стиль (`STD`).
fn read_std(std: &[u8], base_size: usize, word6: bool) -> Option<StyleEntry> {
    let sti = u16_at(std, 0)? & 0x0FFF;
    let word = u16_at(std, 2)?;
    let kind = match word & 0x000F {
        1 => Some(StyleKind::Paragraph),
        2 => Some(StyleKind::Character),
        3 => Some(StyleKind::Table),
        4 => Some(StyleKind::Numbering),
        _ => None,
    };
    let based_on = word >> 4;
    let word = u16_at(std, 4)?;
    let upx_count = usize::from(word & 0x000F);
    let next = word >> 4;

    // Имя: длина в знаках UTF-16, знаки и завершающий ноль (у Word 6 —
    // по байту на знак).
    let (name, mut at) = if word6 {
        crate::word6::style_name(std, base_size)?
    } else {
        let at = base_size;
        let len = usize::from(u16_at(std, at)?);
        let units: Vec<u16> = (0..len)
            .map_while(|i| u16_at(std, at + 2 + i * 2))
            .collect();
        (String::from_utf16_lossy(&units), at + 2 + (len + 1) * 2)
    };

    // Оформление (`UPX`): у стиля абзаца — абзац и знаки, у стиля
    // знаков — только знаки. Каждое выровнено на два байта.
    let mut upx = Vec::new();
    for _ in 0..upx_count {
        if at % 2 == 1 {
            at += 1;
        }
        let Some(cb) = u16_at(std, at).map(usize::from) else {
            break;
        };
        at += 2;
        upx.push(std.get(at..at + cb).unwrap_or_default().to_vec());
        at += cb;
    }
    if word6 {
        for (i, grpprl) in upx.iter_mut().enumerate() {
            // У оформления абзаца впереди номер стиля — его не трогаем.
            let skip = if i == 0 && kind == Some(StyleKind::Paragraph) {
                2.min(grpprl.len())
            } else {
                0
            };
            let converted = crate::word6::convert(&grpprl[skip..]);
            grpprl.truncate(skip);
            grpprl.extend(converted);
        }
    }
    let (papx, chpx) = match (kind, upx.len()) {
        // У оформления абзаца впереди номер стиля — он не нужен.
        (Some(StyleKind::Paragraph), 2..) => {
            (upx[0].get(2..).unwrap_or_default().to_vec(), upx[1].clone())
        }
        (Some(StyleKind::Character), 1..) => (Vec::new(), upx[0].clone()),
        _ => (Vec::new(), Vec::new()),
    };
    Some(StyleEntry {
        sti,
        kind,
        name,
        based_on,
        next,
        papx,
        chpx,
    })
}

/// Английское имя встроенного стиля по номеру `sti` (его Word пишет
/// в .docx), если стиль из тех, что важны для вёрстки и списка стилей.
pub(crate) fn builtin_name(sti: u16) -> Option<&'static str> {
    Some(match sti {
        0 => "Normal",
        1 => "heading 1",
        2 => "heading 2",
        3 => "heading 3",
        4 => "heading 4",
        5 => "heading 5",
        6 => "heading 6",
        7 => "heading 7",
        8 => "heading 8",
        9 => "heading 9",
        29 => "footnote text",
        30 => "annotation text",
        31 => "header",
        32 => "footer",
        38 => "footnote reference",
        62 => "Title",
        65 => "Default Paragraph Font",
        74 => "Subtitle",
        85 => "Hyperlink",
        105 => "Normal Table",
        107 => "No List",
        _ => return None,
    })
}

/// Идентификатор стиля по имени, как его делает Word: слова с большой
/// буквы, без пробелов («heading 1» → «Heading1»).
pub(crate) fn style_id(name: &str) -> String {
    name.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// Имена шрифтов по номерам (`SttbfFfn`, MS-DOC 2.9.286).
pub(crate) fn fonts(bytes: &[u8]) -> Vec<String> {
    let count = usize::from(u16_at(bytes, 0).unwrap_or(0));
    let mut at = 4;
    let mut out = Vec::new();
    for _ in 0..count {
        let Some(&cb) = bytes.get(at) else {
            break;
        };
        let ffn = bytes
            .get(at + 1..at + 1 + usize::from(cb))
            .unwrap_or_default();
        // Имя — с 39-го байта записи, UTF-16 до нуля.
        let name: Vec<u16> = (0..)
            .map_while(|i| u16_at(ffn, 39 + i * 2))
            .take_while(|&c| c != 0)
            .collect();
        out.push(String::from_utf16_lossy(&name));
        at += 1 + usize::from(cb);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn style_ids_like_word() {
        assert_eq!(style_id("heading 1"), "Heading1");
        assert_eq!(style_id("Normal"), "Normal");
        assert_eq!(builtin_name(2), Some("heading 2"));
    }

    #[test]
    fn style_sheet_with_one_paragraph_style() {
        let mut std = Vec::new();
        std.extend(1u16.to_le_bytes()); // sti 1 — заголовок 1
        std.extend(1u16.to_le_bytes()); // абзаца, основан на 0
        std.extend(2u16.to_le_bytes()); // два UPX, следующий 0
        std.extend([0; 4]); // bchUpe, grfstd
        std.extend(9u16.to_le_bytes());
        std.extend(utf16("heading 1"));
        std.extend([0, 0]);
        std.extend(5u16.to_le_bytes()); // UPX абзаца: istd и одно свойство
        std.extend([1, 0, 0x06, 0x24, 1]);
        std.push(0); // выравнивание на два байта
        std.extend(3u16.to_le_bytes()); // UPX знаков: полужирный
        std.extend([0x35, 0x08, 1]);

        let mut stshi = Vec::new();
        stshi.extend(2u16.to_le_bytes()); // два стиля
        stshi.extend(10u16.to_le_bytes());
        stshi.extend([0; 8]);
        stshi.extend([1, 0, 2, 0, 3, 0]);
        let mut bytes = (stshi.len() as u16).to_le_bytes().to_vec();
        bytes.extend(&stshi);
        bytes.extend(0u16.to_le_bytes()); // пустое место
        bytes.extend((std.len() as u16).to_le_bytes());
        bytes.extend(&std);

        let sheet = StyleSheet::read(&bytes, false);
        assert_eq!(sheet.default_fonts, [1, 2, 3]);
        assert!(sheet.get(0).is_none());
        let style = sheet.get(1).unwrap();
        assert_eq!(style.name, "heading 1");
        assert_eq!(style.kind, Some(StyleKind::Paragraph));
        assert_eq!(style.papx, [0x06, 0x24, 1]);
        assert_eq!(style.chpx, [0x35, 0x08, 1]);
    }

    #[test]
    fn font_names() {
        let mut ffn = vec![0u8; 39];
        ffn.extend(utf16("Arial"));
        ffn.extend([0, 0]);
        let mut bytes = vec![1, 0, 0, 0, ffn.len() as u8];
        bytes.extend(&ffn);
        assert_eq!(fonts(&bytes), ["Arial"]);
        assert!(fonts(&[]).is_empty());
    }
}
