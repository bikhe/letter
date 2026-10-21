# letter

Открытый текстовый редактор для Windows, macOS и Linux, написанный на Rust.
Цель — повторить внешний вид, возможности и совместимость с файлами
Microsoft Word (2024 и более ранних версий).

Проект в самом начале пути: сейчас программа открывает пустое окно.
План работ — в [plan.md](plan.md).

## Как собрать и запустить

Понадобится около 2 ГБ свободного места и интернет (первая сборка скачивает
библиотеки).

### 1. Установите Rust

- **Windows:** скачайте и запустите `rustup-init.exe` с сайта
  <https://rustup.rs>. Если установщик попросит поставить
  «Visual Studio C++ Build Tools», согласитесь (нужен вариант
  «Desktop development with C++»).
- **macOS:** откройте «Терминал» и выполните:
  ```sh
  xcode-select --install
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Linux:** в терминале выполните:
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
  Для Ubuntu/Debian также нужны пакеты:
  ```sh
  sudo apt install build-essential pkg-config libxkbcommon-x11-0 \
    libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
  ```

После установки закройте терминал и откройте его заново.

### 2. Скачайте проект

Нажмите на GitHub зелёную кнопку **Code → Download ZIP** и распакуйте архив,
или, если установлен git:

```sh
git clone https://github.com/bikhe/letter.git
cd letter
```

### 3. Запустите

В терминале, находясь в папке проекта:

```sh
cargo run -p letter-app
```

Первая сборка занимает несколько минут, следующие — секунды. Должно открыться
окно с пустой страницей. Файл .docx открывается через «Файл → Открыть»
(Ctrl+O), перетаскиванием в окно или сразу при запуске:
`cargo run -p letter-app -- документ.docx`. Масштаб — Ctrl + колесо мыши
или ползунок внизу окна.

## Для разработчиков

Перед отправкой изменений должны проходить:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

То же самое проверяет CI на Windows, macOS и Linux. Правила работы и
архитектура описаны в [plan.md](plan.md).

Посмотреть, как программа поняла файл .docx, проверить сохранение
раскладку по страницам и нарисовать страницы в картинки PNG:

```sh
cargo run -p letter-cli -- dump документ.docx
cargo run -p letter-cli -- roundtrip документ.docx копия.docx
cargo run -p letter-cli -- pages документ.docx
cargo run -p letter-cli -- render документ.docx папка
```

## Лицензия

[MIT](LICENSE). Проект не связан с Microsoft; «Microsoft Word» — товарный
знак Microsoft Corporation.
