//! Заголовок файла (FIB, MS-DOC 2.5): где в потоках лежат части
//! документа и сколько в нём знаков.

use crate::ReadError;

/// Признак документа Word в начале потока `WordDocument`.
const W_IDENT: u16 = 0xA5EC;

/// Самая ранняя версия формата Word 97 (`nFib`). У Word 6/95 меньше.
const NFIB_WORD97: u16 = 0x00C0;

/// Версии формата Word 6 (`0x65`) и Word 95 (`0x68`), с запасом.
const NFIB_WORD6: std::ops::RangeInclusive<u16> = 0x0065..=0x0069;

/// Сколько пар «смещение — длина» в начале заголовка Word 6/95.
const WORD6_PAIRS: usize = 38;

/// Номера пар «смещение — длина» в `FibRgFcLcb97`, которые нам нужны.
pub(crate) mod index {
    /// Таблица стилей (`STSH`).
    pub const STSHF: usize = 1;
    /// Ссылки на сноски (`PlcffndRef`).
    pub const PLCFFND_REF: usize = 2;
    /// Текст сносок (`PlcffndTxt`).
    pub const PLCFFND_TXT: usize = 3;
    /// Ссылки на примечания (`PlcfandRef`).
    pub const PLCFAND_REF: usize = 4;
    /// Текст примечаний (`PlcfandTxt`).
    pub const PLCFAND_TXT: usize = 5;
    /// Разделы (`PlcfSed`).
    pub const PLCF_SED: usize = 6;
    /// Страницы оформления знаков (`PlcBteChpx`).
    pub const PLCF_BTE_CHPX: usize = 12;
    /// Страницы оформления абзацев (`PlcBtePapx`).
    pub const PLCF_BTE_PAPX: usize = 13;
    /// Закладки: имена (`SttbfBkmk`), начала (`PlcfBkf`) и концы
    /// (`PlcfBkl`).
    pub const STTBF_BKMK: usize = 21;
    pub const PLCF_BKF: usize = 22;
    pub const PLCF_BKL: usize = 23;
    /// Колонтитулы (`PlcfHdd`).
    pub const PLCF_HDD: usize = 11;
    /// Шрифты (`SttbfFfn`).
    pub const STTBF_FFN: usize = 15;
    /// Свойства документа (`Dop`).
    pub const DOP: usize = 31;
    /// Таблица кусков текста (`Clx`).
    pub const CLX: usize = 33;
    /// Авторы примечаний (`GrpXstAtnOwners`).
    pub const GRP_XST_ATN_OWNERS: usize = 36;
    /// Закладки примечаний: имена (`SttbfAtnBkmk`), начала (`PlcfAtnBkf`)
    /// и концы (`PlcfAtnBkl`).
    pub const STTBF_ATN_BKMK: usize = 37;
    /// Плавающие фигуры основного текста (`PlcSpaMom`) и колонтитулов
    /// (`PlcSpaHdr`).
    pub const PLC_SPA_MOM: usize = 40;
    pub const PLC_SPA_HDR: usize = 41;
    pub const PLCF_ATN_BKF: usize = 42;
    pub const PLCF_ATN_BKL: usize = 43;
    /// Ссылки на концевые сноски (`PlcfendRef`) и их текст (`PlcfendTxt`).
    pub const PLCFEND_REF: usize = 46;
    pub const PLCFEND_TXT: usize = 47;
    /// Рисунки документа (`OfficeArtContent`).
    pub const DGG_INFO: usize = 50;
    /// Списки (`PlfLst`, за ним — уровни `LVL`) и их варианты (`PlfLfo`).
    pub const PLF_LST: usize = 73;
    pub const PLF_LFO: usize = 74;
}

/// Части текста документа по порядку (`CP`): основной текст, сноски,
/// колонтитулы, примечания, концевые сноски.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Stories {
    /// Знаков в основном тексте.
    pub text: u32,
    /// Знаков в сносках.
    pub footnotes: u32,
    /// Знаков в колонтитулах.
    pub headers: u32,
    /// Знаков в макросах (не используется, но место занимает).
    pub macros: u32,
    /// Знаков в примечаниях.
    pub comments: u32,
    /// Знаков в концевых сносках.
    pub endnotes: u32,
}

impl Stories {
    /// Начало сносок.
    pub fn footnotes_start(&self) -> u32 {
        self.text
    }

    /// Начало колонтитулов.
    pub fn headers_start(&self) -> u32 {
        self.footnotes_start().saturating_add(self.footnotes)
    }

    /// Начало примечаний.
    pub fn comments_start(&self) -> u32 {
        self.headers_start()
            .saturating_add(self.headers)
            .saturating_add(self.macros)
    }

    /// Начало концевых сносок.
    pub fn endnotes_start(&self) -> u32 {
        self.comments_start().saturating_add(self.comments)
    }

    /// Всего знаков во всех этих частях.
    pub fn total(&self) -> u32 {
        self.endnotes_start().saturating_add(self.endnotes)
    }
}

/// Нужное из заголовка.
#[derive(Debug, Clone)]
pub(crate) struct Fib {
    /// Сколько знаков в каждой части текста.
    pub stories: Stories,
    /// Пары «смещение в табличном потоке — длина».
    pub fc_lcb: Vec<(u32, u32)>,
    /// Особенности Word 6/95; `None` — Word 97 и новее.
    pub word6: Option<Word6>,
}

/// Что есть только в заголовке Word 6/95.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Word6 {
    /// Кодировка текста: в Word 6/95 по байту на знак, кодовая страница
    /// — по языку документа.
    pub encoding: &'static encoding_rs::Encoding,
    /// Начало текста в потоке (`fcMin`), если нет таблицы кусков.
    pub fc_min: u32,
    /// Куски текста — в UTF-16 (признак `fExtChar`; так пишут
    /// некоторые программы), иначе по байту на знак.
    pub unicode: bool,
    /// Первая страница оформления знаков и абзацев и их число
    /// (`pnChpFirst`, `pnPapFirst`, `cpnBteChp`, `cpnBtePap`): таблица
    /// страниц бывает неполной, остальные страницы идут подряд.
    pub pn_chp_first: u16,
    pub pn_pap_first: u16,
    pub cpn_chp: u16,
    pub cpn_pap: u16,
}

/// Документ ли это Word 6/95 (по версии в заголовке).
pub(crate) fn is_word6(stream: &[u8]) -> bool {
    u16_at(stream, 2).is_some_and(|n| NFIB_WORD6.contains(&n))
}

impl Fib {
    /// Разбирает заголовок в начале потока `WordDocument`.
    pub fn read(stream: &[u8]) -> Result<Fib, ReadError> {
        let short = || ReadError::Corrupt("короткий заголовок".into());
        let ident = u16_at(stream, 0).ok_or_else(short)?;
        let n_fib = u16_at(stream, 2).ok_or_else(short)?;
        // У Word 6/95 признак свой (0xA5DC; 0xA699 пишут и другие
        // программы).
        let word6 = NFIB_WORD6.contains(&n_fib) && [0xA5DC, 0xA699, W_IDENT].contains(&ident);
        if ident != W_IDENT && !word6 {
            return Err(ReadError::NotADoc);
        }
        let flags = u16_at(stream, 10).ok_or_else(short)?;
        // Зашифрованный документ сюда попадает уже расшифрованным.
        if flags & 0x0100 != 0 {
            return Err(ReadError::Encrypted);
        }
        if word6 {
            return Fib::read_word6(stream);
        }
        if n_fib < NFIB_WORD97 {
            return Err(ReadError::OldVersion);
        }
        let csw = usize::from(u16_at(stream, 32).ok_or_else(short)?);
        let at = 34 + csw * 2;
        let cslw = usize::from(u16_at(stream, at).ok_or_else(short)?);
        let rg_lw = at + 2;
        let ccp = |i: usize| u32_at(stream, rg_lw + i * 4).unwrap_or(0);
        let stories = Stories {
            text: u32_at(stream, rg_lw + 3 * 4).ok_or_else(short)?,
            footnotes: ccp(4),
            headers: ccp(5),
            macros: ccp(6),
            comments: ccp(7),
            endnotes: ccp(8),
        };
        let at = rg_lw + cslw * 4;
        let count = usize::from(u16_at(stream, at).ok_or_else(short)?);
        let blob = at + 2;
        let fc_lcb = (0..count)
            .map(|i| {
                Some((
                    u32_at(stream, blob + i * 8)?,
                    u32_at(stream, blob + i * 8 + 4)?,
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(short)?;
        Ok(Fib {
            stories,
            fc_lcb,
            word6: None,
        })
    }

    /// Заголовок Word 6/95: всё на постоянных местах, таблицы лежат
    /// в том же потоке `WordDocument`.
    fn read_word6(stream: &[u8]) -> Result<Fib, ReadError> {
        let short = || ReadError::Corrupt("короткий заголовок".into());
        let ccp = |i: usize| u32_at(stream, 0x34 + i * 4).ok_or_else(short);
        let stories = Stories {
            text: ccp(0)?,
            footnotes: ccp(1)?,
            headers: ccp(2)?,
            macros: ccp(3)?,
            comments: ccp(4)?,
            endnotes: ccp(5)?,
        };
        let pair = |at: usize| Some((u32_at(stream, at)?, u32_at(stream, at + 4)?));
        // Пары 0–37 — с 0x58; дальше четыре слова о страницах оформления
        // и снова пары с теми же номерами, что в Word 97 (нужны до 47:
        // закладки примечаний и концевые сноски). На местах 40 и 41
        // в Word 97 фигуры, а в Word 6 — пустые поля.
        let fc_lcb = (0..WORD6_PAIRS)
            .map(|i| pair(0x58 + i * 8))
            .chain((WORD6_PAIRS..=47).map(|i| match i {
                40 | 41 => Some((0, 0)),
                _ => pair(0x192 + (i - WORD6_PAIRS) * 8),
            }))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(short)?;
        let at = 0x58 + WORD6_PAIRS * 8 + 2;
        let word = |i: usize| u16_at(stream, at + i * 2).ok_or_else(short);
        Ok(Fib {
            stories,
            fc_lcb,
            word6: Some(Word6 {
                encoding: encoding(u16_at(stream, 6).unwrap_or(0)),
                fc_min: u32_at(stream, 0x18).ok_or_else(short)?,
                unicode: u16_at(stream, 10).is_some_and(|flags| flags & 0x1000 != 0),
                pn_chp_first: word(0)?,
                pn_pap_first: word(1)?,
                cpn_chp: word(2)?,
                cpn_pap: word(3)?,
            }),
        })
    }

    /// Кусок табличного потока по номеру пары; `None` — части нет или
    /// она выходит за поток.
    pub fn part<'t>(&self, table: &'t [u8], index: usize) -> Option<&'t [u8]> {
        let &(fc, lcb) = self.fc_lcb.get(index)?;
        if lcb == 0 {
            return None;
        }
        table.get(fc as usize..(fc as usize).checked_add(lcb as usize)?)
    }

    /// Табличный поток от начала части до конца: у некоторых частей
    /// (списки) данные продолжаются за указанной длиной.
    pub fn part_and_rest<'t>(&self, table: &'t [u8], index: usize) -> Option<&'t [u8]> {
        let &(fc, lcb) = self.fc_lcb.get(index)?;
        if lcb == 0 {
            return None;
        }
        table.get(fc as usize..)
    }
}

/// Кодовая страница текста по языку (`lid`) документа Word 6/95.
fn encoding(lid: u16) -> &'static encoding_rs::Encoding {
    use encoding_rs::*;
    match (lid & 0x03FF, lid) {
        // Сербский кириллицей.
        (0x1A, 0x0C1A) => WINDOWS_1251,
        // Русский, украинский, белорусский, болгарский, македонский,
        // казахский, киргизский, татарский, монгольский.
        (0x19 | 0x22 | 0x23 | 0x02 | 0x2F | 0x3F | 0x40 | 0x44 | 0x50, _) => WINDOWS_1251,
        // Польский, чешский, словацкий, венгерский, словенский,
        // хорватский, румынский, албанский.
        (0x15 | 0x05 | 0x1B | 0x0E | 0x24 | 0x1A | 0x18 | 0x1C, _) => WINDOWS_1250,
        (0x08, _) => WINDOWS_1253,
        (0x1F, _) => WINDOWS_1254,
        (0x0D, _) => WINDOWS_1255,
        (0x01, _) => WINDOWS_1256,
        (0x25..=0x27, _) => WINDOWS_1257,
        (0x2A, _) => WINDOWS_1258,
        (0x1E, _) => WINDOWS_874,
        (0x11, _) => SHIFT_JIS,
        (0x12, _) => EUC_KR,
        (0x04, 0x0404 | 0x0C04 | 0x1404) => BIG5,
        (0x04, _) => GBK,
        _ => WINDOWS_1252,
    }
}

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Таблица `PLC`: `n + 1` позиций по 4 байта и `n` записей по `size`
/// байт. Возвращает позиции и записи.
pub(crate) fn plc(bytes: &[u8], size: usize) -> Option<(Vec<u32>, Vec<&[u8]>)> {
    if bytes.len() < 4 {
        return None;
    }
    let n = (bytes.len() - 4) / (4 + size);
    let positions = (0..=n)
        .map(|i| u32_at(bytes, i * 4))
        .collect::<Option<Vec<_>>>()?;
    let data = (n + 1) * 4;
    let entries = (0..n)
        .map(|i| bytes.get(data + i * size..data + (i + 1) * size))
        .collect::<Option<Vec<_>>>()?;
    Some((positions, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plc_is_split_into_positions_and_entries() {
        let mut bytes = Vec::new();
        for p in [0u32, 10, 20] {
            bytes.extend(p.to_le_bytes());
        }
        bytes.extend([1, 2, 3, 4, 5, 6]);
        let (positions, entries) = plc(&bytes, 3).unwrap();
        assert_eq!(positions, [0, 10, 20]);
        assert_eq!(entries, [&[1, 2, 3][..], &[4, 5, 6][..]]);
        assert!(plc(&[1, 2], 3).is_none());
    }

    #[test]
    fn stories_follow_each_other() {
        let stories = Stories {
            text: 100,
            footnotes: 10,
            headers: 20,
            macros: 1,
            comments: 5,
            endnotes: 7,
        };
        assert_eq!(stories.footnotes_start(), 100);
        assert_eq!(stories.headers_start(), 110);
        assert_eq!(stories.comments_start(), 131);
        assert_eq!(stories.endnotes_start(), 136);
        assert_eq!(stories.total(), 143);
    }

    #[test]
    fn not_a_word_document() {
        assert!(matches!(Fib::read(&[0; 600]), Err(ReadError::NotADoc)));
        assert!(matches!(Fib::read(&[]), Err(ReadError::Corrupt(_))));
        let mut old = vec![0; 600];
        old[..2].copy_from_slice(&W_IDENT.to_le_bytes());
        // Word 2.0 не читается, а Word 6 — да.
        old[2..4].copy_from_slice(&0x002Du16.to_le_bytes());
        assert!(matches!(Fib::read(&old), Err(ReadError::OldVersion)));
        old[2..4].copy_from_slice(&0x0065u16.to_le_bytes());
        old[6..8].copy_from_slice(&0x0419u16.to_le_bytes());
        assert!(is_word6(&old));
        let fib = Fib::read(&old).unwrap();
        assert_eq!(fib.word6.unwrap().encoding, encoding_rs::WINDOWS_1251);
        assert_eq!(encoding(0x0409), encoding_rs::WINDOWS_1252);
        assert_eq!(encoding(0x041A), encoding_rs::WINDOWS_1250);
        assert_eq!(encoding(0x0C1A), encoding_rs::WINDOWS_1251);
        old[10] = 0;
        old[11] = 0x01;
        assert!(matches!(Fib::read(&old), Err(ReadError::Encrypted)));
    }
}
