# UtilityCrab 🦀

A small Windows tray tool written in Rust that reads selected text aloud, copies text from images and converts or shrinks images.

## Features

- **Read aloud:** select text anywhere and press a hotkey. Speech starts after the first sentence is ready, and pressing the hotkey again stops it.
- **Text from screenshots (OCR):** snip a region of the screen and the recognized text lands in your clipboard; a popup shows what was copied. Uses the built-in Windows OCR.
- **Image converter:** right-click an image in Explorer → **Konvertieren** → convert it to PNG, JPG, WebP or ICO, or **Verkleinern** (shrink) it. Several selected files are handled in one go. Afterwards the new files are on the clipboard (paste them with Ctrl+V into a folder, chat or e-mail) and a small popup at the bottom center of the screen shows the progress in percent and then the result (size change, explanations, a button to show the file). Windows notifications can be chosen instead in the settings. **Weitere Optionen …** opens a window with every setting:
  - target format (PNG, JPG, WebP, BMP, GIF, TIFF, ICO), quality, resizing by longest edge or percent
  - a maximum file size in KB: quality and, if needed, resolution are lowered until the file fits
  - presets for e-mail, websites and "under 1 MB"
  - drag & drop of files and folders; the window can also be opened from the tray menu

  Reads PNG, JPG, WebP, BMP, GIF, TIFF and ICO, plus HEIC and AVIF through Windows codecs (install *HEIF Image Extensions* or *AV1 Video Extension* from the Microsoft Store). Originals are never changed. PNGs are optimized losslessly with [oxipng](https://github.com/shssoichiro/oxipng). Metadata such as GPS position is removed. On Windows 11 the menu is under **Show more options** (or Shift+right-click).
- **Multiple voice engines**, all running locally:
  - **Windows voices:** the voices built into Windows, no download needed.
  - **[Supertonic 3](https://huggingface.co/Supertone/supertonic-3):** natural-sounding neural TTS with 31 languages and 10 voices (~400 MB).
  - **[Piper](https://github.com/rhasspy/piper):** small, fast voices with accurate pronunciation (20–110 MB per voice).
- **Automatic language detection** per sentence. Piper and the Windows voices switch to an installed voice of the detected language.
- **Pronunciation dictionary** for words a voice gets wrong.
- **Settings window** to pick voice and speed, download or delete models, and change the hotkeys.
- **Updates:** installed versions check GitHub Releases once a day and offer a new version in a popup. One click downloads the setup, installs it silently and restarts UtilityCrab. Turn it off under **Info & Updates** in the settings, or check by hand from the tray menu (**Nach Updates suchen**).

## Default hotkeys

| Hotkey | Action |
|---|---|
| `Ctrl+Alt+S` | Read selected text / stop |
| `Ctrl+Alt+O` | Copy text from a screen snip |

Both hotkeys can be changed in the settings window, which you open by right-clicking the tray icon and choosing **Einstellungen …**.

## Building

Requires Windows 10/11 and a Rust toolchain with the MSVC target.

```sh
cargo run --release
```

Use the release build for everyday use. The neural voices are noticeably slower in debug builds.

### Installer

Requires [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`).

```powershell
.\installer\build.ps1
```

This builds the release and writes `dist\UtilityCrab-Setup-<version>.exe`. The setup installs per user (no admin rights), adds a Start menu entry and can register UtilityCrab to start with Windows. The version is also built into the exe (`UTILITYCRAB_VERSION`) for the update check; plain `cargo run` builds have none and don't check automatically.

### Releases

Every push to `main` builds the setup on GitHub Actions and publishes it as release `v<Cargo version>.<run number>`, for example `v0.1.0.7`. Installed copies pick it up through the update check.

## Popups

`src/popup` is a small reusable module for these popups: a `Notice` is built from blocks (progress, before/after stat, text, quote, hint, chip, details) and can be updated from another thread. The tray app shows them through a separate process (`--hinweis`, JSON lines on stdin). To check the design, run:

```sh
utility_crab.exe --hinweis-demo            # all states one after another
utility_crab.exe --hinweis-demo groesser   # one state, stays open
```

## Data

Settings, the error log and downloaded models are stored in `%LOCALAPPDATA%\UtilityCrab`.

## Notes

- The UI is in German.
- Models are downloaded from Hugging Face and GitHub on demand. Check each model's license before redistributing it: Supertonic 3 uses OpenRAIL-M, and Piper voices vary by speaker.
