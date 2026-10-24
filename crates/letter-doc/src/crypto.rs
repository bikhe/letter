//! Документы .doc с паролем на открытие (MS-DOC 2.2.6, MS-OFFCRYPTO 2.3).
//!
//! Word шифрует .doc одним из трёх способов:
//!
//! - **RC4** (Word 97/2000): ключ из MD5 пароля и «соли»;
//! - **RC4 CryptoAPI** (Word 2002–2003 и новее): ключ из SHA-1;
//! - **XOR** («совместимый с Word 6/95», самый слабый): байты смешиваются
//!   с 16-байтовым ключом из пароля.
//!
//! Зашифрованы потоки `WordDocument` (кроме первых 68 байт заголовка),
//! табличный поток (`0Table`/`1Table`) и `Data`. Описание шифра лежит
//! в начале табличного потока (у RC4) или в заголовке (у XOR).

use md5::Md5;
use sha1::{Digest, Sha1};

use crate::ReadError;
use crate::fib::{u16_at, u32_at};

/// Сколько байт в начале потока `WordDocument` не шифруется (`FibBase`).
pub(crate) const PLAIN_HEADER: usize = 68;

/// То же у Word 6/95.
pub(crate) const PLAIN_HEADER_WORD6: usize = 0x34;

/// RC4 меняет ключ через каждые столько байт потока.
const BLOCK: usize = 512;

/// Признаки в заголовке: файл зашифрован; шифр — XOR, а не RC4.
const F_ENCRYPTED: u16 = 0x0100;
const F_OBFUSCATED: u16 = 0x8000;

/// Каким способом зашифрован документ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cipher {
    /// RC4 с ключом из MD5.
    Rc4 {
        salt: [u8; 16],
        verifier: [u8; 16],
        verifier_hash: [u8; 16],
    },
    /// RC4 CryptoAPI с ключом из SHA-1 длиной `key_bits`.
    CryptoApi {
        salt: Vec<u8>,
        key_bits: u32,
        verifier: [u8; 16],
        verifier_hash: [u8; 20],
    },
    /// XOR; `verifier` — проверочное число пароля из заголовка.
    Xor { verifier: u32 },
}

/// Зашифрован ли документ (по потоку `WordDocument`).
pub(crate) fn is_encrypted(word: &[u8]) -> bool {
    u16_at(word, 10).is_some_and(|flags| flags & F_ENCRYPTED != 0)
}

/// Узнаёт шифр по заголовку и началу табличного потока. Word 6/95
/// (`word6`) шифрует только способом XOR.
pub(crate) fn cipher(word: &[u8], table: &[u8], word6: bool) -> Result<Cipher, ReadError> {
    let corrupt = || ReadError::Corrupt("испорчено описание шифра".into());
    let flags = u16_at(word, 10).ok_or_else(corrupt)?;
    let key = u32_at(word, 14).ok_or_else(corrupt)?;
    if word6 || flags & F_OBFUSCATED != 0 {
        return Ok(Cipher::Xor { verifier: key });
    }
    let bytes = |at: usize, len: usize| table.get(at..at + len).ok_or_else(corrupt);
    match (u16_at(table, 0), u16_at(table, 2)) {
        (Some(1), Some(1)) => Ok(Cipher::Rc4 {
            salt: array(bytes(4, 16)?),
            verifier: array(bytes(20, 16)?),
            verifier_hash: array(bytes(36, 16)?),
        }),
        (Some(2..=4), Some(2)) => {
            // Версия, признаки, длина заголовка, заголовок, затем
            // проверочные данные.
            let header = u32_at(table, 8).ok_or_else(corrupt)? as usize;
            let key_bits = match u32_at(table, 12 + 16).ok_or_else(corrupt)? {
                0 => 40,
                bits => bits,
            };
            if !(40..=128).contains(&key_bits) || !key_bits.is_multiple_of(8) {
                return Err(corrupt());
            }
            let at = 12usize.checked_add(header).ok_or_else(corrupt)?;
            let salt_size = u32_at(table, at).ok_or_else(corrupt)? as usize;
            if salt_size != 16 {
                return Err(corrupt());
            }
            Ok(Cipher::CryptoApi {
                salt: bytes(at + 4, 16)?.to_vec(),
                key_bits,
                verifier: array(bytes(at + 20, 16)?),
                verifier_hash: array(bytes(at + 40, 20)?),
            })
        }
        _ => Err(ReadError::Corrupt("неизвестный способ шифрования".into())),
    }
}

fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0; N];
    out.copy_from_slice(&bytes[..N]);
    out
}

/// Расшифровывает потоки паролем; первые `plain` байт потока
/// `WordDocument` не зашифрованы. Неверный пароль —
/// [`ReadError::WrongPassword`], и потоки тогда не меняются.
pub(crate) fn decrypt(
    cipher: &Cipher,
    password: &str,
    plain: usize,
    word: &mut [u8],
    table: &mut [u8],
    data: &mut [u8],
) -> Result<(), ReadError> {
    let header = match word.get(..plain.max(12)) {
        Some(header) => header.to_vec(),
        None => return Err(ReadError::Corrupt("короткий заголовок".into())),
    };
    match cipher {
        Cipher::Rc4 {
            salt,
            verifier,
            verifier_hash,
        } => {
            let base = rc4_base(password, salt);
            let key = |block: u32| -> Vec<u8> {
                let mut md5 = Md5::new();
                md5.update(&base[..5]);
                md5.update(block.to_le_bytes());
                md5.finalize().to_vec()
            };
            let mut check = [verifier.as_slice(), verifier_hash].concat();
            Rc4::new(&key(0)).apply(&mut check);
            if Md5::digest(&check[..16])[..] != check[16..] {
                return Err(ReadError::WrongPassword);
            }
            for stream in [&mut *word, table, data] {
                rc4_stream(stream, &key);
            }
        }
        Cipher::CryptoApi {
            salt,
            key_bits,
            verifier,
            verifier_hash,
        } => {
            let mut sha = Sha1::new();
            sha.update(salt);
            sha.update(utf16(password));
            let base = sha.finalize();
            let key = |block: u32| -> Vec<u8> {
                let mut sha = Sha1::new();
                sha.update(base);
                sha.update(block.to_le_bytes());
                let hash = sha.finalize();
                let mut key = hash[..*key_bits as usize / 8].to_vec();
                // 40-битный ключ дополняется нулями до 128 бит.
                if *key_bits == 40 {
                    key.resize(16, 0);
                }
                key
            };
            let mut check = [verifier.as_slice(), verifier_hash].concat();
            Rc4::new(&key(0)).apply(&mut check);
            if Sha1::digest(&check[..16])[..] != check[16..] {
                return Err(ReadError::WrongPassword);
            }
            for stream in [&mut *word, table, data] {
                rc4_stream(stream, &key);
            }
        }
        Cipher::Xor { verifier } => {
            let password = xor_password(password);
            let (hash, key) = (xor_verifier(&password), xor_key(&password));
            // Слово проверки — в младшей половине числа из заголовка.
            let low = (verifier & 0xFFFF) as u16;
            let high = (verifier >> 16) as u16;
            if low != hash && high != hash {
                return Err(ReadError::WrongPassword);
            }
            let array = xor_array(&password, key);
            for stream in [&mut *word, table, data] {
                xor_stream(stream, &array);
            }
        }
    }
    // Заголовок не шифруется; признак шифра снимаем, чтобы дальше
    // документ читался как обычный.
    word[..header.len()].copy_from_slice(&header);
    let flags = u16::from_le_bytes([word[10], word[11]]) & !(F_ENCRYPTED | F_OBFUSCATED);
    word[10..12].copy_from_slice(&flags.to_le_bytes());
    Ok(())
}

/// Пароль в UTF-16LE (не больше 255 знаков, как в Word).
fn utf16(password: &str) -> Vec<u8> {
    password
        .encode_utf16()
        .take(255)
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Общая часть ключа RC4 (MS-OFFCRYPTO 2.3.6.2).
fn rc4_base(password: &str, salt: &[u8; 16]) -> [u8; 16] {
    let start = Md5::digest(utf16(password));
    let mut md5 = Md5::new();
    for _ in 0..16 {
        md5.update(&start[..5]);
        md5.update(salt);
    }
    array(&md5.finalize())
}

/// Расшифровывает поток блоками по 512 байт: у каждого блока свой ключ.
fn rc4_stream(stream: &mut [u8], key: &impl Fn(u32) -> Vec<u8>) {
    for (block, chunk) in stream.chunks_mut(BLOCK).enumerate() {
        Rc4::new(&key(block as u32)).apply(chunk);
    }
}

/// Шифр RC4.
struct Rc4 {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    fn new(key: &[u8]) -> Rc4 {
        let mut state = [0u8; 256];
        for (i, s) in state.iter_mut().enumerate() {
            *s = i as u8;
        }
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, usize::from(j));
        }
        Rc4 { state, i: 0, j: 0 }
    }

    fn apply(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.state[usize::from(self.i)]);
            self.state.swap(usize::from(self.i), usize::from(self.j));
            let k = self.state[usize::from(
                self.state[usize::from(self.i)].wrapping_add(self.state[usize::from(self.j)]),
            )];
            *byte ^= k;
        }
    }
}

/// Пароль для XOR: по байту на знак (младший, а если он нулевой —
/// старший), не больше 15 знаков (MS-OFFCRYPTO 2.3.7.4).
fn xor_password(password: &str) -> Vec<u8> {
    password
        .encode_utf16()
        .take(15)
        .map(|c| match c.to_le_bytes() {
            [0, high] => high,
            [low, _] => low,
        })
        .collect()
}

/// Проверочное слово пароля (MS-OFFCRYPTO 2.3.7.1).
fn xor_verifier(password: &[u8]) -> u16 {
    let mut verifier = 0u16;
    let len = password.len() as u8;
    for &byte in password.iter().rev().chain(std::iter::once(&len)) {
        verifier = ((verifier >> 14) & 1) | ((verifier << 1) & 0x7FFF);
        verifier ^= u16::from(byte);
    }
    verifier ^ 0xCE4B
}

/// Начальные значения ключа XOR по длине пароля.
const INITIAL_CODE: [u16; 15] = [
    0xE1F0, 0x1D0F, 0xCC9C, 0x84C0, 0x110C, 0x0E10, 0xF1CE, 0x313E, 0x1872, 0xE139, 0xD40F, 0x84F9,
    0x280C, 0xA96A, 0x4EC3,
];

/// Таблица для ключа XOR.
const XOR_MATRIX: [u16; 105] = [
    0xAEFC, 0x4DD9, 0x9BB2, 0x2745, 0x4E8A, 0x9D14, 0x2A09, 0x7B61, 0xF6C2, 0xFDA5, 0xEB6B, 0xC6F7,
    0x9DCF, 0x2BBF, 0x4563, 0x8AC6, 0x05AD, 0x0B5A, 0x16B4, 0x2D68, 0x5AD0, 0x0375, 0x06EA, 0x0DD4,
    0x1BA8, 0x3750, 0x6EA0, 0xDD40, 0xD849, 0xA0B3, 0x5147, 0xA28E, 0x553D, 0xAA7A, 0x44D5, 0x6F45,
    0xDE8A, 0xAD35, 0x4A4B, 0x9496, 0x390D, 0x721A, 0xEB23, 0xC667, 0x9CEF, 0x29FF, 0x53FE, 0xA7FC,
    0x5FD9, 0x47D3, 0x8FA6, 0x0F6D, 0x1EDA, 0x3DB4, 0x7B68, 0xF6D0, 0xB861, 0x60E3, 0xC1C6, 0x93AD,
    0x377B, 0x6EF6, 0xDDEC, 0x45A0, 0x8B40, 0x06A1, 0x0D42, 0x1A84, 0x3508, 0x6A10, 0xAA51, 0x4483,
    0x8906, 0x022D, 0x045A, 0x08B4, 0x1168, 0x76B4, 0xED68, 0xCAF1, 0x85C3, 0x1BA7, 0x374E, 0x6E9C,
    0x3730, 0x6E60, 0xDCC0, 0xA9A1, 0x4363, 0x86C6, 0x1DAD, 0x3331, 0x6662, 0xCCC4, 0x89A9, 0x0373,
    0x06E6, 0x0DCC, 0x1021, 0x2042, 0x4084, 0x8108, 0x1231, 0x2462, 0x48C4,
];

/// Чем дополняется пароль до 16 байт.
const PAD: [u8; 15] = [
    0xBB, 0xFF, 0xFF, 0xBA, 0xFF, 0xFF, 0xB9, 0x80, 0x00, 0xBE, 0x0F, 0x00, 0xBF, 0x0F, 0x00,
];

/// Ключ XOR (MS-OFFCRYPTO 2.3.7.2, `CreateXorKey_Method1`).
fn xor_key(password: &[u8]) -> u16 {
    let Some(&start) = password
        .len()
        .checked_sub(1)
        .and_then(|i| INITIAL_CODE.get(i))
    else {
        return 0;
    };
    let mut key = start;
    let mut element = XOR_MATRIX.len();
    for &byte in password.iter().rev() {
        let mut ch = byte;
        for _ in 0..7 {
            element -= 1;
            if ch & 0x40 != 0 {
                key ^= XOR_MATRIX[element];
            }
            ch <<= 1;
        }
    }
    key
}

/// 16 байт ключа XOR (`CreateXorArray_Method1`).
fn xor_array(password: &[u8], key: u16) -> [u8; 16] {
    let [low, high] = key.to_le_bytes();
    let mix =
        |byte: u8, i: usize| (byte ^ if i.is_multiple_of(2) { low } else { high }).rotate_right(1);
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        let byte = match password.get(i) {
            Some(&b) => b,
            None => PAD.get(i - password.len()).copied().unwrap_or(0),
        };
        *slot = mix(byte, i);
    }
    out
}

/// Снимает XOR с потока: нулевые байты и байты, равные ключу, Word
/// оставляет как есть (`DecryptData_Method2`).
fn xor_stream(stream: &mut [u8], array: &[u8; 16]) {
    for (i, byte) in stream.iter_mut().enumerate() {
        let k = array[i % 16];
        if *byte != 0 && *byte != k {
            *byte ^= k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_matches_known_vector() {
        // Пример из описания RC4: ключ "Key", текст "Plaintext".
        let mut text = *b"Plaintext";
        Rc4::new(b"Key").apply(&mut text);
        assert_eq!(text, [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]);
    }

    #[test]
    fn xor_verifier_matches_known_value() {
        // Пароль Excel «по умолчанию» и его проверочное слово.
        assert_eq!(xor_verifier(b"VelvetSweatshop"), 0x9A0A);
        assert_eq!(xor_password("пароль").len(), 6);
    }

    #[test]
    fn xor_array_matches_reference() {
        // Значения посчитаны независимой реализацией (msoffcrypto-tool).
        let password = b"secret12";
        let key = xor_key(password);
        assert_eq!(key, 0x8AED);
        assert_eq!(
            xor_array(password, key),
            [
                79, 247, 71, 124, 68, 127, 110, 92, 43, 186, 9, 24, 9, 186, 42, 5
            ]
        );
        let key = xor_key(b"abc");
        assert_eq!(key, 0x514A);
        assert_eq!(
            xor_array(b"abc", key),
            [
                149, 153, 148, 117, 218, 87, 120, 87, 218, 116, 101, 168, 122, 47, 37, 119
            ]
        );
        assert_eq!(xor_key(b""), 0);
    }

    #[test]
    fn xor_keeps_zero_and_key_bytes() {
        let array = [7u8; 16];
        let mut bytes = [0, 7, 1, 6];
        xor_stream(&mut bytes, &array);
        assert_eq!(bytes, [0, 7, 6, 1]);
    }
}
