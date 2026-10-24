//! Шифрует .doc способами RC4 CryptoAPI и XOR (MS-OFFCRYPTO 2.3.5,
//! 2.3.7) для тестов: `cargo run -- исходный.doc папка`.
//!
//! Способ RC4 (Word 97/2000) умеет LibreOffice, а этих двух — нет.

use std::io::{Read, Write};

use sha1::{Digest, Sha1};

const PASSWORD: &str = "пароль";
const PLAIN_HEADER: usize = 68;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (source, out) = (&args[1], std::path::Path::new(&args[2]));
    let mut file = cfb::open(source).expect("исходный файл");
    let names: Vec<String> = file
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    let mut streams = Vec::new();
    for name in names {
        let mut bytes = Vec::new();
        file.open_stream(&name)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        streams.push((name, bytes));
    }
    write(
        &out.join("encrypted-cryptoapi.doc"),
        cryptoapi(streams.clone()),
    );
    write(&out.join("encrypted-xor.doc"), xor(streams));
}

fn write(path: &std::path::Path, streams: Vec<(String, Vec<u8>)>) {
    let mut file = cfb::create(path).unwrap();
    for (name, bytes) in streams {
        file.create_stream(&name)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
    file.flush().unwrap();
}

fn table_name(word: &[u8]) -> &'static str {
    if u16::from_le_bytes([word[10], word[11]]) & 0x0200 != 0 {
        "/1Table"
    } else {
        "/0Table"
    }
}

fn find<'a>(streams: &'a mut [(String, Vec<u8>)], name: &str) -> &'a mut Vec<u8> {
    &mut streams.iter_mut().find(|(n, _)| n == name).unwrap().1
}

fn set_flags(word: &mut [u8], add: u16, key: u32) {
    let flags = u16::from_le_bytes([word[10], word[11]]) | add;
    word[10..12].copy_from_slice(&flags.to_le_bytes());
    word[14..18].copy_from_slice(&key.to_le_bytes());
}

fn cryptoapi(mut streams: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let salt: [u8; 16] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    let verifier: [u8; 16] = std::array::from_fn(|i| (i as u8).wrapping_mul(91).wrapping_add(5));
    let pw: Vec<u8> = PASSWORD.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let base = Sha1::digest([salt.as_slice(), &pw].concat());
    let key =
        |block: u32| Sha1::digest([base.as_slice(), &block.to_le_bytes()].concat())[..16].to_vec();

    // Описание шифра: версия 4.2, признаки, заголовок, проверка.
    let mut header = Vec::new();
    for v in [0x04u32, 0, 0x6801, 0x8004, 128, 1, 0, 0] {
        header.extend(v.to_le_bytes());
    }
    for c in "Microsoft Enhanced Cryptographic Provider v1.0\0".encode_utf16() {
        header.extend(c.to_le_bytes());
    }
    let mut check = [verifier.as_slice(), &Sha1::digest(verifier)].concat();
    rc4(&key(0), &mut check);
    let mut info = Vec::new();
    info.extend(4u16.to_le_bytes());
    info.extend(2u16.to_le_bytes());
    info.extend(0x04u32.to_le_bytes());
    info.extend((header.len() as u32).to_le_bytes());
    info.extend(header);
    info.extend(16u32.to_le_bytes());
    info.extend(salt);
    info.extend(&check[..16]);
    info.extend(20u32.to_le_bytes());
    info.extend(&check[16..]);

    let word = find(&mut streams, "/WordDocument");
    let shift = info.len() as u32;
    shift_table(word, shift);
    set_flags(word, 0x0100, shift);
    let table = table_name(word);
    let header_copy = word[..PLAIN_HEADER].to_vec();
    blocks(word, &key);
    word[..PLAIN_HEADER].copy_from_slice(&header_copy);
    let t = find(&mut streams, table);
    let mut new = info.clone();
    new.extend_from_slice(t);
    blocks(&mut new, &key);
    new[..info.len()].copy_from_slice(&info);
    *t = new;
    if streams.iter().any(|(n, _)| n == "/Data") {
        blocks(find(&mut streams, "/Data"), &key);
    }
    streams
}

/// Сдвигает смещения частей в табличном потоке (перед ними встаёт
/// описание шифра).
fn shift_table(word: &mut [u8], shift: u32) {
    let u16_at = |w: &[u8], at: usize| usize::from(u16::from_le_bytes([w[at], w[at + 1]]));
    let csw = u16_at(word, 32);
    let at = 34 + csw * 2;
    let cslw = u16_at(word, at);
    let at = at + 2 + cslw * 4;
    let count = u16_at(word, at);
    for i in 0..count {
        let p = at + 2 + i * 8;
        let fc = u32::from_le_bytes(word[p..p + 4].try_into().unwrap());
        let lcb = u32::from_le_bytes(word[p + 4..p + 8].try_into().unwrap());
        if lcb != 0 {
            word[p..p + 4].copy_from_slice(&(fc + shift).to_le_bytes());
        }
    }
}

fn blocks(stream: &mut [u8], key: &impl Fn(u32) -> Vec<u8>) {
    for (block, chunk) in stream.chunks_mut(512).enumerate() {
        rc4(&key(block as u32), chunk);
    }
}

fn rc4(key: &[u8], bytes: &mut [u8]) {
    let mut s: Vec<u8> = (0..=255).collect();
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    for b in bytes {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        *b ^= s[s[i as usize].wrapping_add(s[j as usize]) as usize];
    }
}

fn xor(mut streams: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    // Ключ и проверочное слово для пароля «secret12» посчитаны
    // msoffcrypto-tool (create_xor_array_method1, verifypw).
    let array: [u8; 16] = [
        79, 247, 71, 124, 68, 127, 110, 92, 43, 186, 9, 24, 9, 186, 42, 5,
    ];
    let verifier = verifier(b"secret12");
    let word = find(&mut streams, "/WordDocument");
    set_flags(word, 0x8100, u32::from(verifier));
    let table = table_name(word);
    let header = word[..PLAIN_HEADER].to_vec();
    obfuscate(word, &array);
    word[..PLAIN_HEADER].copy_from_slice(&header);
    obfuscate(find(&mut streams, table), &array);
    if streams.iter().any(|(n, _)| n == "/Data") {
        obfuscate(find(&mut streams, "/Data"), &array);
    }
    streams
}

fn verifier(password: &[u8]) -> u16 {
    let mut v = 0u16;
    for &b in password
        .iter()
        .rev()
        .chain(std::iter::once(&(password.len() as u8)))
    {
        v = ((v >> 14) & 1) | ((v << 1) & 0x7FFF);
        v ^= u16::from(b);
    }
    v ^ 0xCE4B
}

fn obfuscate(stream: &mut [u8], array: &[u8; 16]) {
    for (i, b) in stream.iter_mut().enumerate() {
        let k = array[i % 16];
        if *b != 0 && *b != k {
            *b ^= k;
        }
    }
}
