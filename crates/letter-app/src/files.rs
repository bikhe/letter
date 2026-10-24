//! Файлы: чтение и запись документов во всех форматах, список недавних
//! файлов и автосохранение для восстановления после сбоя.
//!
//! Настройки и автосохранения лежат в папке программы:
//! Windows — `%APPDATA%\letter`, macOS — `~/Library/Application
//! Support/letter`, Linux — `$XDG_CONFIG_HOME/letter` (или
//! `~/.config/letter`).

use crate::i18n::tr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use letter_model::Document;

/// Формат файла документа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    /// Документ Word (.docx).
    Docx,
    /// Старый документ Word 97–2003 (.doc, .dot) — только чтение.
    Doc,
    /// RTF.
    Rtf,
    /// OpenDocument (.odt, а для чтения ещё и плоский .fodt).
    Odt,
    /// Простой текст, HTML или Markdown.
    Text(letter_text::Format),
}

impl FileFormat {
    /// Формат по расширению файла.
    pub fn from_path(path: &Path) -> Option<FileFormat> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match extension.as_str() {
            "docx" | "docm" | "dotx" | "dotm" => FileFormat::Docx,
            "doc" | "dot" => FileFormat::Doc,
            "rtf" => FileFormat::Rtf,
            "odt" | "fodt" => FileFormat::Odt,
            _ => FileFormat::Text(letter_text::Format::from_path(path)?),
        })
    }

    /// Можно ли записать документ в этом формате.
    pub fn writable(self) -> bool {
        self != FileFormat::Doc
    }
}

/// Фильтры окна «Открыть»: название и расширения.
pub const OPEN_FILTERS: &[(&str, &[&str])] = &[
    (
        "filter-all-documents",
        &[
            "docx", "docm", "dotx", "dotm", "doc", "dot", "rtf", "odt", "fodt", "txt", "html",
            "htm", "md",
        ],
    ),
    ("filter-docx", &["docx", "docm", "dotx", "dotm"]),
    ("filter-doc", &["doc", "dot"]),
    ("filter-rtf", &["rtf"]),
    ("filter-odt", &["odt", "fodt"]),
    ("filter-txt", &["txt"]),
    ("filter-html", &["html", "htm"]),
    ("filter-md", &["md"]),
];

/// Фильтры окна «Сохранить как». Первый — формат по умолчанию.
pub const SAVE_FILTERS: &[(&str, &[&str])] = &[
    ("filter-docx", &["docx"]),
    ("filter-docm", &["docm"]),
    ("filter-dotx", &["dotx"]),
    ("filter-dotm", &["dotm"]),
    ("filter-rtf", &["rtf"]),
    ("filter-odt", &["odt"]),
    ("filter-txt", &["txt"]),
    ("filter-html", &["html"]),
    ("filter-md", &["md"]),
    ("filter-pdf", &["pdf"]),
];

/// Сохранение в PDF — не сохранение документа, а его копия для
/// чтения и печати: путь документа после него не меняется.
pub fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Записывает свёрстанный документ в PDF `path`: страницы со
/// встроенными шрифтами, оглавление из заголовков, ссылки.
pub fn write_pdf(
    doc: &Document,
    layout: &letter_layout::DocumentLayout,
    library: &letter_layout::FontLibrary,
    path: &Path,
) -> Result<(), String> {
    let info = letter_render::PdfInfo {
        title: doc.properties.core.title.clone(),
        author: doc.properties.core.creator.clone(),
        outline: letter_editor::document_outline(doc, layout)
            .into_iter()
            .map(|entry| letter_render::OutlineItem {
                level: entry.level,
                title: entry.title,
                page: entry.page,
                top: entry.top,
            })
            .collect(),
        anchors: letter_editor::bookmark_places(doc, layout),
    };
    let bytes = letter_render::write_pdf(&layout.pages, library, &info);
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Новый пустой документ: стили как у пустого документа Word.
pub fn blank() -> Document {
    letter_text::read_txt(&[])
}

/// Шаблон ли это Word (.dotx, .dotm). Как в Word, шаблон открывается
/// новым документом на его основе, а сам файл шаблона не меняется.
pub fn is_template(path: &Path) -> bool {
    letter_docx::FileKind::from_path(path).is_some_and(letter_docx::FileKind::is_template)
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dot"))
}

/// Читает документ; формат — по расширению.
pub fn read(path: &Path) -> Result<Document, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let format = FileFormat::from_path(path).ok_or_else(|| fail(&tr("error-unknown-format")))?;
    let data = fs::read(path).map_err(|e| fail(&e))?;
    match format {
        FileFormat::Docx => letter_docx::read_bytes(&data).map_err(|e| fail(&e)),
        FileFormat::Doc => letter_doc::read(&data).map_err(|e| fail(&e)),
        FileFormat::Rtf => letter_rtf::read(&data).map_err(|e| fail(&e)),
        FileFormat::Odt => letter_odt::read(&data).map_err(|e| fail(&e)),
        FileFormat::Text(letter_text::Format::Text) => Ok(letter_text::read_txt(&data)),
        FileFormat::Text(letter_text::Format::Html) => Ok(letter_text::read_html(&data)),
        FileFormat::Text(letter_text::Format::Markdown) => Ok(letter_text::read_markdown(&data)),
    }
}

/// Зашифрован ли файл паролем на открытие (.docx и его разновидности,
/// .doc).
pub fn is_encrypted(path: &Path) -> bool {
    match FileFormat::from_path(path) {
        Some(FileFormat::Docx) => fs::read(path).is_ok_and(|data| letter_docx::is_encrypted(&data)),
        Some(FileFormat::Doc) => fs::read(path).is_ok_and(|data| letter_doc::is_encrypted(&data)),
        _ => false,
    }
}

/// Читает документ, зашифрованный паролем (см. [`is_encrypted`]).
pub fn read_with_password(path: &Path, password: &str) -> Result<Document, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let data = fs::read(path).map_err(|e| fail(&e))?;
    if FileFormat::from_path(path) == Some(FileFormat::Doc) {
        return letter_doc::read_with_password(&data, password).map_err(|e| fail(&e));
    }
    letter_docx::read_bytes_with_password(&data, password).map_err(|e| fail(&e))
}

/// Записывает документ; формат — по расширению. Сначала пишется
/// временный файл рядом, потом он заменяет прежний: если программа
/// упадёт посреди записи, старый файл останется целым.
///
/// Файл Word (.docx, .docm, .dotx, .dotm) с паролем `password`
/// шифруется. Другие форматы шифровать нельзя, и пароль для них не
/// используется.
pub fn write(doc: &Document, path: &Path, password: Option<&str>) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let format = FileFormat::from_path(path).ok_or_else(|| fail(&tr("error-unknown-format")))?;
    let data = match format {
        // Разновидность (.docx, .docm, .dotx, .dotm) — по расширению: Word
        // не откроет файл, у которого они не совпадают.
        FileFormat::Docx => match letter_docx::FileKind::from_path(path) {
            Some(kind) => letter_docx::write_bytes_as(doc, kind),
            None => letter_docx::write_bytes(doc),
        }
        .and_then(|bytes| match password {
            Some(password) => letter_docx::encrypt_bytes(&bytes, password),
            None => Ok(bytes),
        })
        .map_err(|e| fail(&e))?,
        FileFormat::Doc => return Err(fail(&tr("error-read-only-format"))),
        FileFormat::Rtf => letter_rtf::write(doc).into_bytes(),
        FileFormat::Odt => letter_odt::write(doc).map_err(|e| fail(&e))?,
        FileFormat::Text(letter_text::Format::Text) => letter_text::write_txt(doc),
        FileFormat::Text(letter_text::Format::Html) => letter_text::write_html(doc).into_bytes(),
        FileFormat::Text(letter_text::Format::Markdown) => {
            letter_text::write_markdown(doc).into_bytes()
        }
    };
    write_atomic(path, &data).map_err(|e| fail(&e))
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!("~${name}.tmp"));
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Имя папки рядом с программой, которая включает портативный режим.
pub const PORTABLE_DIR: &str = "letter-data";

/// Папка программы для настроек и автосохранений. В портативной версии
/// (рядом с программой есть папка [`PORTABLE_DIR`]) — она, чтобы всё
/// хранилось на флешке вместе с программой.
pub fn app_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    if let Some(dir) = exe.as_deref().and_then(portable_dir) {
        return Some(dir);
    }
    system_app_dir()
}

/// Папка портативного режима рядом с программой `exe`, если она есть.
fn portable_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?.join(PORTABLE_DIR);
    dir.is_dir().then_some(dir)
}

/// Обычная папка настроек в профиле пользователя.
fn system_app_dir() -> Option<PathBuf> {
    let env = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?
    };
    Some(base.join("letter"))
}

/// Сколько недавних файлов помнить.
const RECENT_LIMIT: usize = 10;

/// Список недавних файлов (меню «Файл → Последние»). Хранится в
/// `recent.txt`, по пути на строку, новые сверху.
#[derive(Debug, Clone, Default)]
pub struct Recent {
    file: Option<PathBuf>,
    items: Vec<PathBuf>,
}

impl Recent {
    /// Читает список из папки программы `dir` (`None` — не хранить).
    pub fn load(dir: Option<&Path>) -> Recent {
        let file = dir.map(|d| d.join("recent.txt"));
        let items = file
            .as_ref()
            .and_then(|f| fs::read_to_string(f).ok())
            .map(|text| {
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(PathBuf::from)
                    .take(RECENT_LIMIT)
                    .collect()
            })
            .unwrap_or_default();
        Recent { file, items }
    }

    /// Файлы, новые сверху.
    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    /// Ставит файл в начало списка и сохраняет список.
    pub fn add(&mut self, path: &Path) {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        self.items.retain(|p| p != &path);
        self.items.insert(0, path);
        self.items.truncate(RECENT_LIMIT);
        self.save();
    }

    /// Убирает файл из списка (например, его больше нет).
    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|p| p != path);
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let text: String = self
            .items
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect();
        // Список недавних — не главное: если записать не вышло, молчим.
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = write_atomic(file, text.as_bytes());
    }
}

/// Автосохранение одного запущенного экземпляра программы. Пока
/// программа работает, она держит замок на файле `<номер>.lock`; если
/// программа упала, замок снимается, и при следующем запуске
/// автосохранения можно восстановить.
#[derive(Debug)]
pub struct Autosave {
    dir: PathBuf,
    id: String,
    /// Держит замок, пока программа работает.
    _lock: File,
}

/// Документ, оставшийся после сбоя.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    /// Автосохранённый файл (.docx).
    pub autosave: PathBuf,
    /// Откуда документ был открыт (`None` — новый, не сохранённый).
    pub original: Option<PathBuf>,
    /// Когда сохранён.
    pub saved: Option<SystemTime>,
}

impl Autosave {
    /// Начинает автосохранение в папке `dir` (подпапка `autosave`
    /// папки программы). `None`, если папку не создать.
    pub fn start(dir: &Path) -> Option<Autosave> {
        fs::create_dir_all(dir).ok()?;
        let id = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        );
        let lock = File::create(dir.join(format!("{id}.lock"))).ok()?;
        lock.try_lock().ok()?;
        Some(Autosave {
            dir: dir.to_owned(),
            id,
            _lock: lock,
        })
    }

    fn files(&self) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{}.docx", self.id)),
            self.dir.join(format!("{}.path", self.id)),
        )
    }

    /// Сохраняет копию документа. `original` — откуда он открыт.
    pub fn save(&self, doc: &Document, original: Option<&Path>) -> Result<(), String> {
        let (copy, note) = self.files();
        let data = letter_docx::write_bytes(doc).map_err(|e| e.to_string())?;
        write_atomic(&copy, &data).map_err(|e| e.to_string())?;
        let original = original
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        write_atomic(&note, original.as_bytes()).map_err(|e| e.to_string())
    }

    /// Убирает копию: документ сохранён или закрыт без изменений.
    pub fn clear(&self) {
        let (copy, note) = self.files();
        let _ = fs::remove_file(copy);
        let _ = fs::remove_file(note);
    }

    /// Документы, оставшиеся от упавших экземпляров программы: их замки
    /// никто не держит.
    pub fn leftovers(dir: &Path) -> Vec<Recovered> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("docx") {
                continue;
            }
            let lock = path.with_extension("lock");
            // Замок держит работающая программа — это не наше.
            if let Ok(file) = File::options().write(true).open(&lock)
                && file.try_lock().is_err()
            {
                continue;
            }
            let original = fs::read_to_string(path.with_extension("path"))
                .ok()
                .filter(|p| !p.trim().is_empty())
                .map(|p| PathBuf::from(p.trim()));
            let saved = entry.metadata().and_then(|m| m.modified()).ok();
            found.push(Recovered {
                autosave: path,
                original,
                saved,
            });
        }
        found.sort_by_key(|r| std::cmp::Reverse(r.saved));
        found
    }

    /// Удаляет оставшийся после сбоя документ (восстановлен или не нужен).
    pub fn discard(recovered: &Recovered) {
        let path = &recovered.autosave;
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(path.with_extension("path"));
        let _ = fs::remove_file(path.with_extension("lock"));
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        // Программа закрылась как надо: копия не нужна.
        self.clear();
        let _ = fs::remove_file(self.dir.join(format!("{}.lock", self.id)));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use letter_model::{Block, Paragraph, Run, RunProps};

    /// Пустая временная папка для теста.
    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "letter-test-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn portable_dir_is_next_to_the_program() {
        let dir = temp_dir("portable");
        let exe = dir.join("letter.exe");
        assert_eq!(portable_dir(&exe), None);
        fs::create_dir(dir.join(PORTABLE_DIR)).unwrap();
        assert_eq!(portable_dir(&exe), Some(dir.join(PORTABLE_DIR)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pdf_copy_is_written() {
        let dir = temp_dir("pdf");
        let path = dir.join("Отчёт.PDF");
        assert!(is_pdf(&path) && !is_pdf(&dir.join("a.docx")));
        let doc = sample();
        let library = letter_layout::FontLibrary::embedded_only();
        let layout =
            letter_layout::layout_document(&doc, &mut letter_layout::Shaper::new(), &library);
        write_pdf(&doc, &layout, &library, &path).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(SAVE_FILTERS.iter().any(|(_, e)| e.contains(&"pdf")));
        let _ = fs::remove_dir_all(dir);
    }

    fn sample() -> Document {
        let mut doc = blank();
        doc.sections[0].blocks = vec![
            Paragraph::from_runs([
                Run::text("Привет, "),
                Run::text("мир").with_props(RunProps {
                    bold: Some(true),
                    ..RunProps::default()
                }),
            ])
            .into(),
            Paragraph::text("второй").into(),
        ];
        doc
    }

    #[test]
    fn formats_by_extension() {
        let format = |p: &str| FileFormat::from_path(Path::new(p));
        assert_eq!(format("a.DOCX"), Some(FileFormat::Docx));
        assert_eq!(format("a.rtf"), Some(FileFormat::Rtf));
        assert_eq!(format("a.fodt"), Some(FileFormat::Odt));
        assert_eq!(
            format("a.md"),
            Some(FileFormat::Text(letter_text::Format::Markdown))
        );
        assert_eq!(format("a.pdf"), None);
        assert_eq!(format("a"), None);
    }

    #[test]
    fn encrypted_file_needs_its_password() {
        let dir = temp_dir("encrypted");
        let path = dir.join("тайна.docx");
        write(&sample(), &path, Some("пароль")).unwrap();
        assert!(is_encrypted(&path));
        assert!(read(&path).is_err());
        assert!(read_with_password(&path, "не тот").is_err());
        let back = read_with_password(&path, "пароль").unwrap();
        assert_eq!(back.plain_text().trim(), "Привет, мир\nвторой");
        // Зашифрованный .doc (его сохранил LibreOffice) тоже спрашивает
        // пароль.
        let doc = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/doc/encrypted-rc4.doc");
        assert!(is_encrypted(&doc) && read(&doc).is_err());
        assert!(read_with_password(&doc, "не тот").is_err());
        assert!(read_with_password(&doc, "пароль").is_ok());
        // В другие форматы пароль не пишется.
        let rtf = dir.join("тайна.rtf");
        write(&sample(), &rtf, Some("пароль")).unwrap();
        assert!(!is_encrypted(&rtf) && read(&rtf).is_ok());
    }

    #[test]
    fn old_word_documents_are_read_only() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/doc/table.doc");
        assert_eq!(FileFormat::from_path(&path), Some(FileFormat::Doc));
        assert!(!FileFormat::Doc.writable() && FileFormat::Docx.writable());
        let doc = read(&path).unwrap();
        assert!(doc.plain_text().contains("R1C1"));
        let dir = temp_dir("doc");
        assert!(write(&doc, &dir.join("copy.doc"), None).is_err());
        assert!(is_template(Path::new("a.DOT")));
    }

    #[test]
    fn word_file_kind_follows_the_extension() {
        let dir = temp_dir("kinds");
        for kind in letter_docx::FileKind::ALL {
            let path = dir.join(format!("doc.{}", kind.extension()));
            write(&sample(), &path, None).unwrap();
            assert_eq!(letter_docx::kind_of(&read(&path).unwrap()), kind);
            assert_eq!(is_template(&path), kind.is_template());
        }
        assert!(!is_template(Path::new("a.rtf")));
    }

    #[test]
    fn blank_document_has_one_empty_paragraph_and_styles() {
        let doc = blank();
        assert_eq!(doc.plain_text(), "");
        assert!(matches!(doc.blocks().next(), Some(Block::Paragraph(_))));
        assert!(doc.styles.get("Normal").is_some());
    }

    #[test]
    fn every_format_round_trips_its_text() {
        let dir = temp_dir("formats");
        for ext in [
            "docx", "docm", "dotx", "dotm", "rtf", "odt", "txt", "html", "md",
        ] {
            let path = dir.join(format!("doc.{ext}"));
            write(&sample(), &path, None).unwrap();
            let back = read(&path).unwrap();
            assert_eq!(back.plain_text().trim(), "Привет, мир\nвторой", "{ext}");
            // Временный файл не остался.
            let left: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("~$"))
                .collect();
            assert!(left.is_empty(), "{ext}");
        }
        assert!(read(&dir.join("missing.docx")).is_err());
        assert!(write(&sample(), &dir.join("doc.pdf"), None).is_err());
        fs::write(dir.join("bad.docx"), b"not a zip").unwrap();
        assert!(read(&dir.join("bad.docx")).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_files_newest_first() {
        let dir = temp_dir("recent");
        let mut recent = Recent::load(Some(&dir));
        assert!(recent.items().is_empty());
        for i in 0..12 {
            recent.add(Path::new(&format!("/nowhere/{i}.docx")));
        }
        recent.add(Path::new("/nowhere/5.docx"));
        let again = Recent::load(Some(&dir));
        assert_eq!(again.items().len(), RECENT_LIMIT);
        assert_eq!(again.items()[0], Path::new("/nowhere/5.docx"));
        assert_eq!(again.items()[1], Path::new("/nowhere/11.docx"));
        recent.remove(Path::new("/nowhere/5.docx"));
        assert_eq!(
            Recent::load(Some(&dir)).items()[0],
            Path::new("/nowhere/11.docx")
        );
        // Без папки — просто в памяти.
        let mut memory = Recent::load(None);
        memory.add(Path::new("/x.docx"));
        assert_eq!(memory.items().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn autosave_survives_only_a_crash() {
        let dir = temp_dir("autosave");
        let running = Autosave::start(&dir).unwrap();
        running
            .save(&sample(), Some(Path::new("/docs/report.docx")))
            .unwrap();
        // Пока программа работает, её копия — не «оставшаяся».
        assert!(Autosave::leftovers(&dir).is_empty());

        // «Сбой»: копия и замок есть, но замок никто не держит.
        let crashed = dir.join("1-1.docx");
        fs::write(&crashed, letter_docx::write_bytes(&sample()).unwrap()).unwrap();
        fs::write(dir.join("1-1.path"), "").unwrap();
        fs::write(dir.join("1-1.lock"), "").unwrap();
        let found = Autosave::leftovers(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].original, None);
        let doc = read(&found[0].autosave).unwrap();
        assert_eq!(doc.plain_text(), "Привет, мир\nвторой");
        Autosave::discard(&found[0]);
        assert!(Autosave::leftovers(&dir).is_empty());

        // Закрылась как надо — копии нет.
        drop(running);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(dir);
    }
}
