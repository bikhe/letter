# Тестовые файлы

Здесь лежат документы, на которых проверяется чтение, запись и вёрстка.

## `docx/`

| Файл | Что внутри |
|---|---|
| `empty.docx` | Пустой документ |
| `plain-text.docx` | Несколько абзацев обычного текста |
| `bold-italic.docx` | Жирный, курсив, подчёркнутый, зачёркнутый, цвет, размер, индексы |
| `headings.docx` | Заголовок документа и заголовки 1–3 уровней |
| `lists.docx` | Маркированный, нумерованный и вложенный списки |
| `table.docx` | Таблица 3×3 с объединённой ячейкой |
| `image.docx` | Встроенная картинка PNG |
| `floating.docx` | Плавающие рисунки: «вокруг рамки» и «за текстом» |
| `sections.docx` | Разделы: на текущей странице с двумя колонками, альбомный, с нечётной страницы |
| `notes.docx` | Две сноски и концевая сноска (с чертами-разделителями) |
| `comments.docx` | Два примечания: на первое есть ответ, второе отмечено «решено» (части `comments.xml` и `commentsExtended.xml`, как у Word) |
| `cells.docx` | Таблица без стиля: свои границы и заливка ячеек, строка-заголовок, высота строк «ровно» и «не меньше» |
| `borders.docx` | Границы и заливка абзацев (рамка вокруг двух абзацев, линия снизу, заливка), цвет и рамка страницы |
| `watermark.docx` | Подложка «ОБРАЗЕЦ» (фигура VML с текстом в верхнем колонтитуле), как её вставляет Word |
| `shapes.docx` | Фигуры DrawingML, как их пишет Word (`mc:AlternateContent`): надпись, стрелка, пунктирный овал за текстом, линия со стрелкой и свой контур в строке, группа |
| `smartart.docx` | SmartArt «Простой процесс» (три блока со стрелками) с готовым видом `diagrams/drawing1.xml`, как его сохраняет Word |
| `charts.docx` | Диаграммы в строке: гистограмма (два ряда, заголовок, легенда снизу), круговая (легенда справа), график с отрицательными значениями; в частях диаграмм только кеш значений, без таблицы с данными |
| `links.docx` | Гиперссылки на сайт и на закладку, закладка вокруг слова |
| `headers.docx` | Колонтитулы: первой страницы, чётных и нечётных; второй раздел наследует верхний колонтитул и имеет свой нижний |
| `header-footer.docx` | Верхний и нижний колонтитулы |
| `ru-en.docx` | Русский и английский текст, типографские символы |
| `math.docx` | Формулы OMML, как их вставляет Word: в строке текста и отдельные (дробь, корень, сумма, интеграл, предел, матрица) |
| `pages.docx` | Три главы на нескольких страницах: разрыв страницы, заголовки, «не разрывать абзац» |

**Источник:** все файлы созданы скриптом [`docx/generate.py`](docx/generate.py)
с помощью библиотеки [python-docx](https://github.com/python-openxml/python-docx)
(лицензия MIT). Текст и картинка придуманы нами. Файлы распространяются
по лицензии проекта (MIT).

Чтобы пересоздать файлы:

```sh
pip install python-docx pillow
python3 tests/fixtures/docx/generate.py
```

Файлы, сохранённые настоящим Microsoft Word, добавим позже: для каждого
обязательно указать здесь, откуда он взят и под какой лицензией.

## `reference/`

PDF-эталоны для сверки вёрстки с Word (задача F2.8): откуда они взяты и
как добавить новые — в [`reference/README.md`](reference/README.md).
Файлы в `reference/word/` сохранены из наших `docx/` программой Microsoft
Word 2024 (сделал владелец проекта), в `reference/libreoffice/` — программой
LibreOffice; все распространяются по лицензии проекта (MIT).

## `rtf/`

Файлы RTF для проверки чтения (задача F8.3): `plain-text`, `bold-italic`,
`ru-en`, `table`, `headings`, `lists`, `pages`. Они сохранены программой
LibreOffice из одноимённых файлов `docx/` и распространяются по лицензии
проекта (MIT). Чтобы пересоздать:

```sh
cd tests/fixtures/rtf
soffice --headless --convert-to rtf ../docx/имя.docx
```

## `odt/`

Файлы ODT для проверки чтения (задача F8.4): `plain-text`, `bold-italic`,
`ru-en`, `table`, `headings`, `lists`, `pages`, `header-footer`. Они
сохранены программой LibreOffice из одноимённых файлов `docx/` и
распространяются по лицензии проекта (MIT). Чтобы пересоздать:

```sh
cd tests/fixtures/odt
soffice --headless --convert-to odt ../docx/имя.docx
```

## Шифрованные документы (`encrypted/`)

Лежат отдельно: это не ZIP-архивы, а составные файлы OLE, и общие тесты
папки `docx/` их не открывают.

| Файл | Что проверяет |
|---|---|
| `ru-en.docx` | `docx/ru-en.docx`, зашифрованный паролем `пароль` (агильное шифрование, AES-256, SHA-512) |

**Источник:** зашифрован программой
[msoffcrypto-tool](https://github.com/nolze/msoffcrypto-tool) (лицензия MIT)
из нашего `docx/ru-en.docx`; распространяется по лицензии проекта (MIT).

## Word 97–2003 (`doc/`)

`plain-text`, `bold-italic`, `headings`, `lists`, `table`, `ru-en`, `pages`,
`header-footer`, `headers`, `notes`, `comments`, `sections`, `borders`, `cells`, `links`, `image`, `floating` — наши файлы из `docx/`, сохранённые LibreOffice в формате
Word 97–2003:

```sh
cd tests/fixtures/doc
soffice --headless --convert-to doc:"MS Word 97" ../docx/имя.docx
```

Распространяются по лицензии проекта (MIT).

### Зашифрованные (`doc/encrypted-*.doc`)

Копии `image.doc` с паролем на открытие:

- `encrypted-rc4.doc` — способ RC4 (как в Word 97/2000), пароль «пароль».
  Сохранён LibreOffice из `docx/image.docx` (параметр `Password` при
  сохранении в фильтр `MS Word 97`).
- `encrypted-cryptoapi.doc` — RC4 CryptoAPI (как в Word 2002 и новее),
  пароль «пароль»;
- `encrypted-xor.doc` — XOR (совместимый с Word 6/95), пароль `secret12`.

Последние два LibreOffice делать не умеет; их сделала маленькая программа
[`doc/encrypt`](doc/encrypt) по описанию MS-OFFCRYPTO (CryptoAPI проверен
расшифровкой в [msoffcrypto-tool](https://github.com/nolze/msoffcrypto-tool),
ключ XOR — им же):

```sh
cd tests/fixtures/doc/encrypt
cargo run -- ../image.doc ..
```

Распространяются по лицензии проекта (MIT).

### Word 6.0 и Word 95 (`doc/word6/`)

Файлы из тестов проекта [Apache POI](https://github.com/apache/poi)
(`test-data/document/`), лицензия Apache 2.0 (текст лицензии —
[`LICENSE-2.0.txt`](doc/word6/LICENSE-2.0.txt), уведомление проекта —
[`NOTICE-apache-poi.txt`](doc/word6/NOTICE-apache-poi.txt)). Ожидаемый
текст в тестах сверен с LibreOffice.

| Файл | Исходное имя в Apache POI | Что в нём |
|------|---------------------------|-----------|
| `word6.doc` | `Word6.doc` | Одна строка текста, Word 6 |
| `formatting.doc` | `Word95.doc` | Четыре абзаца, красный и синий полужирный текст |
| `sections.doc` | `Word6_sections.doc` | Три раздела |
| `headers.doc` | `Word6_sections2.doc` | Word 95: лист A4, полужирные заголовки по центру, нижний колонтитул с полем |
| `czech.doc` | `Bug60936.doc` | Чешский текст (центральноевропейская кодировка шрифта) |
| `russian.doc` | `Bug60942b.doc` | Русский текст (кириллица шрифта) |
| `tables.doc` | `Bug49933.doc` | Word 95: прайс-лист на русском, четыре таблицы |
| `unicode.doc` | `Bug51944.doc` | Записан не Word: куски текста в UTF-16 (признак `fExtChar`) |
