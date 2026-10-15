<p align="center">
  <img src="docs/assets/hero.png" alt="Lariksa: meeting notes that never leave your laptop. The app showing a finished notulen with summary, decisions and action items." width="100%">
</p>

<p align="center">
  <b>Portable meeting notes for Windows.</b> Record any Meet, Zoom or Teams call; when you hang up, Lariksa writes the transcript and the <i>notulen</i> (meeting minutes) with AI that runs on your own machine.<br>
  No bot in the call · no subscription · nothing uploaded.
</p>

<p align="center">
  <a href="https://github.com/daefery/lariksa/releases/latest/download/Lariksa-portable.zip"><img alt="Download portable zip" src="https://img.shields.io/badge/Download-Portable%20.zip-C9F27A?style=for-the-badge&logo=windows&logoColor=17241E&labelColor=C9F27A&color=23362C"></a>
  <a href="https://github.com/daefery/lariksa/releases/latest/download/Lariksa.exe"><img alt="Download Lariksa.exe" src="https://img.shields.io/badge/Download-Single%20.exe-23362C?style=for-the-badge&logo=windows&logoColor=C9F27A"></a>
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-23362C"></a>
  <img alt="Platform: Windows 10/11" src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-23362C">
  <img alt="Rust + Tauri" src="https://img.shields.io/badge/Rust-Tauri-23362C">
  <img alt="Offline AI" src="https://img.shields.io/badge/AI-100%25%20local-C9F27A?labelColor=23362C">
</p>

---

## How it works

![Four steps: record two tracks during the call; after you stop, transcribe with Whisper, write the notulen with a local LLM, then review and share via WhatsApp, Word, Markdown or search.](docs/assets/flow.png)

1. **Record.** Press **Rekam**, or **Ctrl+Shift+R** from inside Zoom, Meet or Teams. Your microphone ("Saya") and the meeting audio from your speakers ("Peserta") are saved as two separate tracks. No AI runs during the call, so it stays smooth. A small always-on-top indicator shows the timer, live levels and a **Selesai** button, and never takes focus from the meeting; press the shortcut again or click **Selesai** to stop. While the notulen is being written, the same indicator (and the taskbar icon) shows the stage and progress.
2. **Transcribe.** Press **Selesai**. Whisper large-v3-turbo (whisper.cpp) turns both tracks into text, merges them by time and drops the mic's echo of the speakers. The track tells it who spoke, so no speaker-detection model is needed.
3. **Write the notulen.** A local LLM (Qwen3-4B via llama.cpp) reads the transcript as numbered lines and answers with line numbers, so every chapter, action item, decision and open question points back to the exact moment it came from. Code, not the model, turns those references into timestamps, matches owners to real speakers, keeps a deadline only if it was actually said, and drops anything that cites a line that does not exist. Whisper is unloaded first, so an 8 GB laptop copes. Optional: in **Pengaturan → Yang menulis notulen**, point it at any OpenAI-compatible API (OpenAI, a gateway such as CLIProxyAPI, or a server on your network) with an address, key and model instead; the 2.5 GB local model is then not needed and can be removed from the same screen.
4. **Review and share.** Action items are grouped by owner with checkboxes; every item has a timestamp that jumps to that moment in the transcript and plays the audio from there. Chapters sit in a side rail, with talk time per speaker. Edit anything inline, then copy it to WhatsApp or export to Word or Markdown. Every meeting is searchable.

Already have a transcript from Read.ai, Google Meet, Zoom or Teams? Import the `.txt` or `.vtt` and Lariksa writes its notulen from it, no audio needed.

You can start the next meeting while the previous one is still processing; jobs run one at a time, in order. If the app closes mid-meeting, the recording is kept and processing resumes on the next start.

## What you get

![Six capability cards: private by design, any call with no bot, Indonesian and English, notulen not just a summary, a glossary for names, portable and resilient.](docs/assets/features.png)

## The notulen speaks the language of the meeting

![The same meeting as Indonesian notulen (Ringkasan, Keputusan, PIC) and as English notulen (Summary, Decisions, Owner).](docs/assets/languages.png)

Indonesian with English work terms ("Jadi kita need to align soal deadline-nya") stays Indonesian. An English meeting gets English notulen, headings included, on screen and in every export. You can also pin one language in **Pengaturan** (Settings). The app itself (menus, buttons, messages) can be shown in Bahasa Indonesia or English: **Pengaturan → Bahasa tampilan**.

## The app

![Six screens: one-time setup, recording with two level meters, processing progress, the transcript, settings, and the dark theme.](docs/assets/screens.png)

<details>
<summary>Full-size screenshots</summary>

| | |
|---|---|
| ![Home](docs/assets/screen-home.png) | ![Recording](docs/assets/screen-recording.png) |
| **Home.** Recent meetings and the record button. | **Recording.** Live levels for you and the meeting. |
| ![Notulen](docs/assets/screen-notulen.png) | ![Transcript](docs/assets/screen-transcript.png) |
| **Notulen.** Click any text to edit; it saves automatically. | **Transcript.** Who said what, with timestamps. |
| ![Settings](docs/assets/screen-settings.png) | ![Dark theme](docs/assets/screen-dark.png) |
| **Settings.** Display language, meeting languages, model, glossary, downloads. | **Dark theme.** |

</details>

All screenshots show fictional sample data, rendered from the real UI code.

## Speed

Measured on an RTX 3050 Ti laptop (4 GB VRAM), i7-12650H, 10 minutes of real Indonesian speech:

| Step | NVIDIA GPU | CPU only |
|---|---|---|
| Transcription (Whisper large-v3-turbo, whisper.cpp) | **36 s** | 630 s |
| Notulen (Qwen3-4B Q4_K_M) | **48 s** | ~200 s |

Both run on the GPU through Vulkan, so any recent NVIDIA, AMD or Intel graphics helps; on a laptop with two GPUs Lariksa picks the faster, discrete one. A 31-minute recorded meeting (both tracks) took 86 s to transcribe and 200 s to write the notulen on that laptop.

## Download

| | File | Size | |
|---|---|---|---|
| **Recommended** | [**Lariksa-portable.zip**](https://github.com/daefery/lariksa/releases/latest/download/Lariksa-portable.zip) | 4 MB | Extract anywhere, run `Lariksa\Lariksa.exe`. Includes the licence files. |
| Single file | [**Lariksa.exe**](https://github.com/daefery/lariksa/releases/latest/download/Lariksa.exe) | 8 MB | The same app as one file. Put it in its own folder: models and meetings are saved next to it. |

Both links always point to the [latest release](https://github.com/daefery/lariksa/releases/latest); checksums are in [`SHA256SUMS.txt`](https://github.com/daefery/lariksa/releases/latest/download/SHA256SUMS.txt). Or [build it yourself](#build-from-source).

**Requirements:** Windows 10 2004+ or Windows 11 (x64) with the WebView2 runtime (built into Windows 11 and current Windows 10), 8 GB RAM (16 GB recommended), about 4 GB of free disk. A graphics card is optional and makes transcription about 17x faster.

## Get started

1. Download one of the files above. No installer, no admin rights: Documents or an external drive both work.
2. Run `Lariksa.exe`. If Windows shows "Windows protected your PC", choose **More info → Run anyway** (the app is not code-signed yet).
3. Click **Unduh semua** (download all) once, about 3.1 GB. After that Lariksa works offline.
4. In **Pengaturan**, add your colleagues' names and team jargon to the glossary.
5. Tell participants the meeting is recorded, press **Rekam**, and **Selesai** when you're done.

Everything (models, runtime and your meetings in `data/`) lives in the app's folder. Copy it to another laptop and your meetings come with it.

## Architecture

![Architecture: Windows audio feeds the recorder, then transcription (whisper.cpp) and notulen (llama-server), both on the GPU through Vulkan, then exports; a service worker, SQLite with full-text search, pinned downloads and a Tauri window support them.](docs/assets/architecture.png)

Two Rust crates: `core/` is everything except the window and is tested on any OS; `desktop/` is the Tauri app that wraps it.

| Module (`core/src/`) | Responsibility |
|---|---|
| `recorder.rs`, `audio.rs` | WASAPI two-track capture, 16 kHz mono, wall-clock padding, crash repair; decoding imported audio/video |
| `transcribe.rs`, `hardware.rs` | whisper.cpp with Indonesian defaults; one GPU choice for both engines |
| `transcript.rs`, `transcript_import.rs` | Utterances, track merge, echo removal; Read.ai / Meet / Zoom / VTT import |
| `language.rs` | Which language the notulen is written in |
| `minutes.rs`, `minutes_writer.rs`, `llm.rs` | Line-referenced notulen and the guardrails around a 4B model; llama-server lifecycle |
| `pipeline.rs` | Transcribe then summarise, with injected engines |
| `store.rs` | SQLite + FTS5 search, settings, glossary |
| `service.rs`, `overlay.rs`, `hotkey.rs` | Worker queue, downloads, recovery; recording pill and shortcut logic |
| `render.rs`, `export.rs`, `media.rs` | WhatsApp, Markdown and DOCX; the audio player's local server |
| `catalog.rs` | Pinned, SHA-256-verified, resumable downloads |
| `assets/prompts.json` | Every prompt, JSON schema and on-screen label, in Indonesian and English |
| `desktop/` | The two windows, the JavaScript API (`bridge.js`), clipboard and file dialogs |
| `ui/` | HTML/CSS/JS with the Lariksa Brand Kit tokens, Plus Jakarta Sans, DM Mono |

## Build from source

Lariksa builds from Linux or WSL for Windows, with no Visual Studio; see [docs/BUILD.md](docs/BUILD.md) for the one-time toolchain setup.

```bash
cargo test -p lariksa-core                # 114 tests on any OS, plus a WASAPI test on Windows
packaging/build.sh                        # tests, then dist/Lariksa/ and dist/release/ (zip, exe, SHA256SUMS.txt)
python3 scripts/preview.py out/           # UI screenshots in headless Chrome with a stub API
python3 docs/visuals/render.py            # regenerate the README visuals
```

Models and runtimes download inside the app on first run.

## Brand

<img src="brand/logo/lockup-light.svg" alt="Lariksa logo" height="56">

The mark is a soft “l” that turns into a written line (*larik*, a line of writing) and ends in a lime full stop: the moment the minutes are done. Logo files, usage rules and the generator are in [`brand/`](brand/README.md).

## Privacy and consent

Audio, transcripts and notulen stay in the app folder. By default the network is used only to download models and runtimes. If you choose an API to write the notulen, the transcript text (never the audio) is sent to the address you entered; the API key is stored in the app folder and never shown again on screen. Recording other people may require their consent where you live; the app reminds you to tell participants.

## Licence

Code: [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for third-party attributions.

The Lariksa name, logo and icon are not covered by the code licence; see [TRADEMARKS.md](TRADEMARKS.md). Forks are welcome under their own name.

Third-party components, none of which are redistributed in this repository except the fonts:

- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT) and [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT), downloaded at first run; [Tauri](https://tauri.app) (MIT/Apache-2.0), [rusqlite](https://github.com/rusqlite/rusqlite) with SQLite (MIT, public domain), [Symphonia](https://github.com/pdeljanov/Symphonia) (MPL-2.0), [wasapi-rs](https://github.com/HEnquist/wasapi-rs) (MIT); the full list of Rust crates and licences is in `Cargo.lock`
- Models downloaded at first run: Whisper large-v3-turbo in ggml format (MIT), Silero VAD (MIT), Qwen3-4B-Instruct-2507 (Apache-2.0).
- Fonts: Plus Jakarta Sans and DM Mono, SIL Open Font License 1.1 (`ui/fonts/`).
