//! Чтение старого двоичного формата .doc (Word 97–2003, MS-DOC, а также
//! Word 6.0 и Word 95).
//!
//! Файл .doc — составной файл OLE. В потоке `WordDocument` лежат
//! заголовок (FIB) и сам текст, в потоке `0Table` или `1Table` —
//! таблицы, которые описывают, где какой кусок текста и как он оформлен.
//!
//! Сейчас читается: текст основного документа (таблица кусков, в том
//! числе после «быстрого сохранения»), шрифты, стили, оформление знаков
//! (шрифт, размер, начертание, подчёркивание, цвет, маркер, индексы,
//! регистр, скрытый текст, интервал) и абзацев (выравнивание, отступы,
//! интервалы, табуляция, «не отрывать» и т. п.), разрывы строк, страниц
//! и колонок, поля (код и результат), таблицы с ширинами столбцов,
//! разделы с размером листа, полями, началом и колонками.
//!
//! Читаются и колонтитулы, сноски, примечания, списки, границы и
//! заливка, картинки, гиперссылки, закладки и сведения о документе.
//!
//! Документы Word 6.0 и Word 95 читаются так же, но в них нет
//! нумерованных списков нового вида, картинок и рисунков: пока
//! читаются текст, оформление, стили, разделы, колонтитулы, сноски,
//! таблицы и закладки.
//!
//! Пока не читаются: фигуры и надписи, исправления; файлы Word 2.0 и
//! старше.
//!
//! Документ с паролем на открытие (RC4, RC4 CryptoAPI или XOR)
//! читается функцией [`read_with_password`].
//!
//! ```no_run
//! let doc = letter_doc::read_file("старый.doc")?;
//! println!("{}", doc.plain_text());
//! # Ok::<(), letter_doc::ReadError>(())
//! ```

mod crypto;
mod fib;
mod links;
mod lists;
mod pictures;
mod properties;
mod reader;
mod sprm;
mod styles;
mod word6;

use std::path::Path;

use letter_model::Document;

/// Почему не удалось прочитать файл .doc.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// Ошибка чтения с диска.
    #[error("не удалось прочитать файл: {0}")]
    Io(#[from] std::io::Error),
    /// Это не документ Word 97–2003 (не составной файл или в нём нет
    /// потока `WordDocument`).
    #[error("это не документ Word 97–2003")]
    NotADoc,
    /// Документ Word 2.0 или ещё более старой версии.
    #[error("документ слишком старой версии Word (до Word 6) не поддерживается")]
    OldVersion,
    /// Документ зашифрован: чтобы его открыть, нужен пароль
    /// (см. [`read_with_password`]).
    #[error("документ защищён паролем")]
    Encrypted,
    /// Пароль не подходит.
    #[error("неверный пароль")]
    WrongPassword,
    /// Файл повреждён.
    #[error("файл .doc повреждён: {0}")]
    Corrupt(String),
}

/// Читает документ .doc из байтов.
///
/// Битый файл никогда не роняет программу: функция возвращает ошибку.
pub fn read(data: &[u8]) -> Result<Document, ReadError> {
    reader::read(data, None)
}

/// Читает документ .doc, зашифрованный паролем `password` (см.
/// [`is_encrypted`]). Незашифрованный документ читается как обычно.
///
/// Неверный пароль даёт [`ReadError::WrongPassword`].
pub fn read_with_password(data: &[u8], password: &str) -> Result<Document, ReadError> {
    reader::read(data, Some(password))
}

/// Зашифрован ли документ .doc паролем на открытие.
pub fn is_encrypted(data: &[u8]) -> bool {
    let Ok(mut file) = reader::open(data) else {
        return false;
    };
    let mut head = [0u8; 12];
    file.open_stream("/WordDocument")
        .and_then(|mut stream| std::io::Read::read_exact(&mut stream, &mut head))
        .is_ok_and(|()| crypto::is_encrypted(&head))
}

/// Читает файл .doc.
pub fn read_file(path: impl AsRef<Path>) -> Result<Document, ReadError> {
    read(&std::fs::read(path)?)
}

/// Похож ли файл на .doc: составной файл OLE с потоком `WordDocument`.
pub fn is_doc(data: &[u8]) -> bool {
    reader::open(data).is_ok_and(|file| file.is_stream("/WordDocument"))
}
