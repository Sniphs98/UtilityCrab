# UtilityCrab 🦀

A small Windows tray tool written in Rust that reads selected text aloud and copies text from images.

## Features

- **Read aloud:** select text anywhere and press a hotkey. Speech starts after the first sentence is ready, and pressing the hotkey again stops it.
- **Text from screenshots (OCR):** snip a region of the screen and the recognized text lands in your clipboard. Uses the built-in Windows OCR.
- **Multiple voice engines**, all running locally:
  - **Windows voices:** the voices built into Windows, no download needed.
  - **[Supertonic 3](https://huggingface.co/Supertone/supertonic-3):** natural-sounding neural TTS with 31 languages and 10 voices (~400 MB).
  - **[Piper](https://github.com/rhasspy/piper):** small, fast voices with accurate pronunciation (20–110 MB per voice).
- **Automatic language detection** per sentence. Piper and the Windows voices switch to an installed voice of the detected language.
- **Pronunciation dictionary** for words a voice gets wrong.
- **Settings window** to pick voice and speed, download or delete models, and change the hotkeys.

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

## Data

Settings, the error log and downloaded models are stored in `%LOCALAPPDATA%\UtilityCrab`.

## Notes

- The UI is in German.
- Models are downloaded from Hugging Face and GitHub on demand. Check each model's license before redistributing it: Supertonic 3 uses OpenRAIL-M, and Piper voices vary by speaker.
