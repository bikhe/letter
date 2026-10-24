//! Старый формат Word 6.0 и Word 95.
//!
//! Он похож на Word 97: те же куски текста, страницы оформления (`FKP`),
//! разделы и колонтитулы. Отличия:
//!
//! - все таблицы лежат в самом потоке `WordDocument`;
//! - текст — по байту на знак; кодировка — по набору знаков шрифта;
//! - свойства (`sprm`) записаны однобайтовыми номерами. Здесь они
//!   переводятся в номера Word 97, и дальше документ читается как обычно;
//! - имена стилей и шрифтов — однобайтовые строки с длиной впереди;
//! - колонтитулы записаны только те, что есть (см. [`expand_headers`]).

use encoding_rs::Encoding;

use crate::fib::u16_at;

/// Длина значения старого свойства.
#[derive(Clone, Copy)]
enum Len {
    /// Ровно столько байт.
    Fixed(usize),
    /// Длина — байт впереди значения.
    Var,
    /// Длина — два байта впереди (`sprmTDefTable`), на единицу больше.
    Var2,
}

/// Длина значения по старому номеру свойства (Word 6). `None` — номер
/// неизвестен: дальше свойства разобрать нельзя.
fn len(sprm: u8) -> Option<Len> {
    use Len::*;
    Some(match sprm {
        0 | 52 | 83 | 163 => Fixed(0),
        2 | 4..=11 | 13 | 14 | 24 | 25 | 29 | 37 | 44 | 50 | 51 => Fixed(1),
        16..=19 | 21 | 22 | 26..=28 | 30..=36 | 38..=43 | 45..=49 => Fixed(2),
        20 => Fixed(4),
        3 | 12 | 15 | 23 | 64 => Var,
        65..=67 | 71 | 75 => Fixed(1),
        68 | 74 | 79 | 81 | 82 | 103 | 105 | 106 | 108 | 113 | 115 | 116 => Var,
        69 | 72 | 80 | 93 | 96 | 97 | 99 | 101 | 107 | 109..=112 => Fixed(2),
        70 => Fixed(4),
        73 | 95 => Fixed(3),
        85..=92 | 94 | 98 | 100 | 102 | 104 | 117..=119 => Fixed(1),
        120 => Fixed(12),
        121..=124 => Fixed(2),
        131 | 132 | 138 | 139 | 142 | 143 | 146 | 147 | 150..=153 | 158 | 159 | 162 => Fixed(1),
        133 | 179 | 181 | 207 => Var,
        136 | 137 => Fixed(3),
        140 | 141 | 144 | 145 | 148 | 149 | 154..=157 | 160 | 161 | 164..=171 => Fixed(2),
        182..=184 | 189 | 195 | 197 | 198 => Fixed(2),
        185 | 186 => Fixed(1),
        187 => Fixed(12),
        188 | 191 => Var,
        190 => Var2,
        192 | 194 | 196 | 200 => Fixed(4),
        193 | 199 => Fixed(5),
        _ => return None,
    })
}

/// Номер свойства Word 97 с тем же значением.
fn same(sprm: u8) -> Option<u16> {
    Some(match sprm {
        // Абзац.
        5 => 0x2403,
        7 => 0x2405,
        8 => 0x2406,
        9 => 0x2407,
        14 => 0x240C,
        15 => 0xC60D,
        16 => 0x840E,
        17 => 0x840F,
        18 => 0x4610,
        19 => 0x8411,
        20 => 0x6412,
        21 => 0xA413,
        22 => 0xA414,
        23 => 0xC615,
        24 => 0x2416,
        25 => 0x2417,
        26 => 0x8418,
        27 => 0x8419,
        28 => 0x841A,
        29 => 0x261B,
        37 => 0x2423,
        44 => 0x242A,
        45 => 0x442B,
        46 => 0x442C,
        47 => 0x442D,
        48 => 0x842E,
        49 => 0x842F,
        51 => 0x2431,
        // Знаки.
        65 => 0x0800,
        66 => 0x0801,
        67 => 0x0802,
        68 => 0x6A03,
        71 => 0x0806,
        75 => 0x080A,
        80 => 0x4A30,
        85 => 0x0835,
        86 => 0x0836,
        87 => 0x0837,
        88 => 0x0838,
        89 => 0x0839,
        90 => 0x083A,
        91 => 0x083B,
        92 => 0x083C,
        94 => 0x2A3E,
        96 => 0x8840,
        98 => 0x2A42,
        99 => 0x4A43,
        101 => 0x4845,
        104 => 0x2A48,
        107 => 0x484B,
        117 => 0x0855,
        118 => 0x0856,
        // Раздел.
        136 => 0xF203,
        137 => 0xF204,
        138 => 0x3005,
        142 => 0x3009,
        143 => 0x300A,
        144 => 0x500B,
        145 => 0x900C,
        147 => 0x300E,
        150 => 0x3011,
        152 => 0x3013,
        153 => GPRF_IHDT,
        154 => 0x5015,
        155 => 0x9016,
        156 => 0xB017,
        157 => 0xB018,
        158 => 0x3019,
        159 => 0x301A,
        160 => 0x501B,
        161 => 0x501C,
        162 => 0x301D,
        164 => 0xB01F,
        165 => 0xB020,
        166 => 0xB021,
        167 => 0xB022,
        168 => 0x9023,
        169 => 0x9024,
        170 => 0xB025,
        171 => 0x5026,
        // Таблица.
        182 => 0x5400,
        183 => 0x9601,
        184 => 0x9602,
        185 => 0x3403,
        186 => 0x3404,
        189 => 0x9407,
        191 => 0xD609,
        192 => 0x740A,
        194 => 0x7621,
        195 => 0x5622,
        196 => 0x7623,
        197 => 0x5624,
        198 => 0x5625,
        200 => 0x7627,
        _ => return None,
    })
}

/// Какие колонтитулы есть у раздела (`sprmSGprfIhdt`). В Word 97 этого
/// свойства нет; номер взят свободный, его разбирает только этот модуль.
pub(crate) const GPRF_IHDT: u16 = 0x3014;

/// Переводит свойства Word 6 в свойства Word 97. Неизвестные и ненужные
/// пропускаются; на неизвестном номере разбор кончается (длину его
/// значения не узнать).
pub(crate) fn convert(grpprl: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(&sprm) = grpprl.get(at) {
        let Some(len) = len(sprm) else {
            break;
        };
        let (start, size) = match len {
            Len::Fixed(n) => (at + 1, n),
            Len::Var => match grpprl.get(at + 1) {
                Some(&n) => (at + 2, usize::from(n)),
                None => break,
            },
            Len::Var2 => match u16_at(grpprl, at + 1) {
                Some(n) => (at + 3, usize::from(n).saturating_sub(1)),
                None => break,
            },
        };
        let Some(op) = grpprl.get(start..start + size) else {
            break;
        };
        at = start + size;
        let mut push = |code: u16, op: &[u8]| {
            out.extend(code.to_le_bytes());
            out.extend(op);
        };
        match sprm {
            // Шрифт — сразу для всех письменностей.
            93 => {
                for code in [0x4A4F, 0x4A50, 0x4A51] {
                    push(code, op);
                }
            }
            // Границы абзаца: двухбайтовые `BRC` → четырёхбайтовые `Brc80`.
            38..=43 => {
                let code = [0x6424, 0x6425, 0x6426, 0x6427, 0x6428, 0x6629][usize::from(sprm - 38)];
                push(code, &brc80(u16_at(op, 0).unwrap_or(0)));
            }
            // Границы таблицы: шесть `BRC`.
            187 => {
                let mut value = vec![24];
                for i in 0..6 {
                    value.extend(brc80(u16_at(op, i * 2).unwrap_or(0)));
                }
                push(0xD605, &value);
            }
            // Границы части ячеек.
            193 => {
                let mut value = vec![7];
                value.extend(op.get(..3).unwrap_or(&[0, 0, 0]));
                value.extend(brc80(u16_at(op, 3).unwrap_or(0)));
                push(0xD620, &value);
            }
            // Ширины ячеек и их границы: ячейка `TC` 10 байт → 20.
            190 => {
                let value = def_table(op);
                let mut code = 0xD608u16.to_le_bytes().to_vec();
                code.extend((value.len() as u16 + 1).to_le_bytes());
                code.extend(value);
                out.extend(code);
            }
            _ => {
                if let Some(code) = same(sprm) {
                    // У переменных свойств Word 97 длина — байт впереди.
                    let var = matches!(len, Len::Var) && code >> 13 == 6;
                    out.extend(code.to_le_bytes());
                    if var {
                        out.push(op.len() as u8);
                    }
                    out.extend(op);
                }
            }
        }
    }
    out
}

/// Граница Word 6 (`BRC`, 2 байта: толщина в 0,75 пт, вид, тень, цвет,
/// отступ) → граница Word 97 (`Brc80`, 4 байта).
fn brc80(brc: u16) -> [u8; 4] {
    let width = (brc & 0x7) as u8;
    let kind = ((brc >> 3) & 0x3) as u8;
    let shadow = (brc >> 5) & 1 != 0;
    let ico = ((brc >> 6) & 0x1F) as u8;
    let space = ((brc >> 11) & 0x1F) as u8;
    if brc == 0 || (kind == 0 && width == 0) {
        return [0; 4];
    }
    let (width, kind) = match width {
        // 6 и 7 — точки и штрихи тонкой линией.
        6 => (6, 6),
        7 => (6, 7),
        w => (w.max(1) * 6, if kind == 0 { 1 } else { kind }),
    };
    [width, kind, ico, space | if shadow { 0x20 } else { 0 }]
}

/// Значение `sprmTDefTable`: ячейки Word 6 (`TC`: флаги и четыре `BRC`)
/// переписываются в ячейки Word 97 (`TC80`: флаги, ширина и четыре
/// `Brc80`).
fn def_table(op: &[u8]) -> Vec<u8> {
    let count = usize::from(op.first().copied().unwrap_or(0));
    let edges = 1 + (count + 1) * 2;
    let mut out = op.get(..edges).unwrap_or(op).to_vec();
    for i in 0..count {
        let Some(tc) = op.get(edges + i * 10..edges + (i + 1) * 10) else {
            break;
        };
        out.extend(&tc[..2]);
        out.extend([0, 0]);
        for side in 0..4 {
            out.extend(brc80(u16_at(tc, 2 + side * 2).unwrap_or(0)));
        }
    }
    out
}

/// Строка с длиной-байтом впереди (`St` Word 6), без завершающего нуля.
fn pascal(bytes: &[u8], encoding: &'static Encoding) -> Option<(String, usize)> {
    let len = usize::from(*bytes.first()?);
    let text = bytes.get(1..1 + len)?;
    let (text, _) = encoding.decode_without_bom_handling(text);
    Some((text.trim_end_matches('\0').to_owned(), 1 + len))
}

/// Имя стиля Word 6: длина-байт, знаки и ноль. Возвращает имя и где
/// кончается запись имени.
pub(crate) fn style_name(std: &[u8], at: usize) -> Option<(String, usize)> {
    let (name, size) = pascal(std.get(at..)?, encoding_rs::WINDOWS_1252)?;
    Some((name, at + size + 1))
}

/// Шрифт Word 6: имя и набор знаков (`chs`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Font {
    pub name: String,
    pub charset: u8,
}

/// Шрифты Word 6 (`SttbfFfn`): общая длина, потом записи `FFN` —
/// длина без единицы, признаки, толщина, набор знаков, место второго
/// имени и имя до нуля.
pub(crate) fn fonts(bytes: &[u8]) -> Vec<Font> {
    let total = usize::from(u16_at(bytes, 0).unwrap_or(0)).min(bytes.len());
    let mut at = 2;
    let mut out = Vec::new();
    while at < total {
        let size = usize::from(bytes[at]) + 1;
        let ffn = bytes.get(at + 1..at + size).unwrap_or_default();
        let charset = ffn.get(3).copied().unwrap_or(0);
        let raw: Vec<u8> = ffn
            .get(5..)
            .unwrap_or_default()
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .collect();
        let (name, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(&raw);
        out.push(Font {
            name: name.into_owned(),
            charset,
        });
        at += size;
    }
    out
}

/// Кодировка однобайтового текста по набору знаков шрифта; `None` —
/// набор не однобайтовый или неизвестный.
pub(crate) fn charset_encoding(charset: u8) -> Option<&'static Encoding> {
    use encoding_rs::*;
    Some(match charset {
        0 | 1 => WINDOWS_1252,
        0xCC => WINDOWS_1251,
        0xEE => WINDOWS_1250,
        0xA1 => WINDOWS_1253,
        0xA2 => WINDOWS_1254,
        0xB1 => WINDOWS_1255,
        0xB2 => WINDOWS_1256,
        0xBA => WINDOWS_1257,
        0xA3 => WINDOWS_1258,
        0xDE => WINDOWS_874,
        _ => return None,
    })
}

/// Таблица строк Word 6 (`STTBF`): общая длина и строки с длиной-байтом.
pub(crate) fn sttb(bytes: &[u8], encoding: &'static Encoding) -> Vec<String> {
    let total = usize::from(u16_at(bytes, 0).unwrap_or(0)).min(bytes.len());
    let mut at = 2;
    let mut out = Vec::new();
    while at < total {
        let Some((text, size)) = pascal(&bytes[at..total], encoding) else {
            break;
        };
        out.push(text);
        at += size;
    }
    out
}

/// Строки с длиной-байтом подряд (авторы примечаний).
pub(crate) fn pascal_list(bytes: &[u8], encoding: &'static Encoding) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((text, size)) = bytes.get(at..).and_then(|b| pascal(b, encoding)) {
        out.push(text);
        at += size;
    }
    out
}

/// Колонтитулы Word 6 записаны подряд только те, что есть: какие —
/// говорят признаки `grpfIhdt` в свойствах документа (черты сносок)
/// и каждого раздела (шесть колонтитулов). Здесь границы раскладываются
/// по местам, как в Word 97: шесть мест на черты и по шесть на раздел;
/// пустое место — колонтитул не задан (взять у прошлого раздела).
pub(crate) fn expand_headers(positions: &[u32], document: u8, sections: &[u8]) -> Vec<u32> {
    let mut ends = positions.iter().skip(1).copied();
    let mut last = positions.first().copied().unwrap_or(0);
    let mut out = vec![last];
    let flags = std::iter::once(document).chain(sections.iter().copied());
    for flags in flags {
        for bit in 0..6 {
            if flags & (1 << bit) != 0
                && let Some(end) = ends.next()
            {
                last = end;
            }
            out.push(last);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprm::sprms;

    #[test]
    fn properties_become_word97_ones() {
        // По центру, рамка сверху, полужирный, шрифт 4, кегль 12.
        let old = [5, 1, 38, 0x09, 0x08, 85, 1, 93, 4, 0, 99, 24, 0];
        let new = convert(&old);
        let parsed = sprms(&new);
        assert_eq!(parsed[0], (0x2403, &[1][..]));
        assert_eq!(parsed[1], (0x6424, &[6, 1, 0, 1][..]));
        assert_eq!(parsed[2], (0x0835, &[1][..]));
        assert_eq!(parsed[3], (0x4A4F, &[4, 0][..]));
        assert_eq!(parsed[6], (0x4A43, &[24, 0][..]));
        // Неизвестный номер — конец разбора.
        assert!(convert(&[255, 1, 2]).is_empty());
        // Переменная длина сохраняется.
        let tabs = convert(&[15, 3, 0, 1, 0]);
        assert_eq!(sprms(&tabs), [(0xC60D, &[0, 1, 0][..])]);
    }

    #[test]
    fn table_cells_are_widened() {
        // Две ячейки: границы 0, 1000, 2000; у первой рамка сверху.
        let mut op = vec![2];
        for x in [0i16, 1000, 2000] {
            op.extend(x.to_le_bytes());
        }
        op.extend([0, 0, 0x09, 0x08, 0, 0, 0, 0, 0, 0]);
        op.extend([0; 10]);
        let mut old = vec![190];
        old.extend((op.len() as u16 + 1).to_le_bytes());
        old.extend(&op);
        let new = convert(&old);
        let parsed = sprms(&new);
        assert_eq!(parsed.len(), 1);
        let (code, value) = parsed[0];
        assert_eq!(code, 0xD608);
        assert_eq!(value.len(), 7 + 2 * 20);
        assert_eq!(&value[7 + 4..7 + 8], &[6, 1, 0, 1]);
    }

    #[test]
    fn font_and_string_tables() {
        let mut ffn = vec![0x16, 0x90, 0x01, 0xCC, 0];
        ffn.extend(b"Arial Cyr\0");
        let mut bytes = vec![0, 0, ffn.len() as u8];
        bytes.extend(&ffn);
        let total = bytes.len() as u16;
        bytes[..2].copy_from_slice(&total.to_le_bytes());
        let fonts = fonts(&bytes);
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].name, "Arial Cyr");
        assert_eq!(
            charset_encoding(fonts[0].charset),
            Some(encoding_rs::WINDOWS_1251)
        );
        assert!(charset_encoding(0x80).is_none());

        let mut sttb_bytes = vec![0, 0, 3];
        sttb_bytes.extend(b"one");
        sttb_bytes.push(2);
        sttb_bytes.extend([0xCF, 0xF0]);
        let total = sttb_bytes.len() as u16;
        sttb_bytes[..2].copy_from_slice(&total.to_le_bytes());
        assert_eq!(sttb(&sttb_bytes, encoding_rs::WINDOWS_1251), ["one", "Пр"]);
        assert_eq!(
            pascal_list(&[1, b'a', 2, b'b', b'c'], encoding_rs::WINDOWS_1252),
            ["a", "bc"]
        );
    }

    #[test]
    fn headers_are_put_into_their_places() {
        // Черт сносок нет; у раздела — верхний и нижний колонтитулы
        // нечётных страниц.
        let positions = [0, 5, 9, 10];
        let expanded = expand_headers(&positions, 0, &[0x02 | 0x08]);
        assert_eq!(expanded.len(), 13);
        assert_eq!(&expanded[..7], &[0; 7]);
        // Слоты раздела: чётный верхний пуст, нечётный верхний 0..5,
        // чётный нижний пуст, нечётный нижний 5..9.
        assert_eq!(&expanded[6..], &[0, 0, 5, 5, 9, 9, 9]);
    }
}
