# Lariksa

Portable meeting notes for Windows that never leave your laptop. Press **Rekam** (record) in any Google Meet, Zoom or Teams call; when you stop, Lariksa writes the transcript and the *notulen* (meeting minutes): summary, decisions, action items with owner and deadline, and open questions. Copy it to WhatsApp or export it to DOCX or Markdown.

No bot joins the call, there is no subscription, and nothing is uploaded. The UI is in Indonesian; the notulen is written in the language of the meeting (Indonesian or English).

## How it works

```
mic  ──► mic.wav    ──┐                    ┌─► notulen (JSON schema) ─► UI / DOCX / MD / WhatsApp
                      ├─► faster-whisper ──┤
loopback ► system.wav ┘   (turbo, VAD)     └─► transcript "Saya" / "Peserta"
             WASAPI            CUDA or CPU            llama.cpp + Qwen3-4B (Vulkan dGPU, iGPU or CPU)
```

1. **Record.** Two tracks: the microphone is you ("Saya"), the system loopback is everyone else ("Peserta"). No AI runs during the call.
2. **Transcribe** after you stop, with Whisper large-v3-turbo. The two tracks are merged by time; the mic's echo of the speakers is dropped, so no diarization model is needed. A glossary of names and jargon biases recognition.
3. **Write the notulen** with a local LLM. Whisper is unloaded first, so an 8 GB laptop copes. The notulen language follows what was spoken, detected from the transcript, or can be fixed in settings.

Everything (models, runtime, and `data/` with your meetings) lives next to `Lariksa.exe`. Copy the folder and your meetings come with it.

## Requirements

- Windows 10 2004+ or Windows 11, x64, with WebView2 (built into Windows 11)
- 8 GB RAM minimum, 16 GB recommended; about 6 GB of disk for the app and models
- Optional NVIDIA GPU: transcription is about 9x faster than on CPU

## Measured

RTX 3050 Ti laptop (4 GB VRAM), i7-12650H, 10 minutes of real Indonesian speech:

| Step | GPU | CPU only |
|---|---|---|
| Transcription, Whisper large-v3-turbo | 21 s | 198 s |
| Notulen, Qwen3-4B Q4_K_M | 48 s | ~200 s |

Estimated, not yet measured: a 1-hour meeting needs about 5 minutes after you stop, on that GPU.

## Build from source

On Windows, with no system Python needed (uv and Python are fetched into `.tools/`):

```powershell
powershell -ExecutionPolicy Bypass -File packaging\build.ps1
```

This runs the tests and produces `dist\Lariksa\Lariksa.exe` and `dist\Lariksa-portable.zip` (~96 MB). Models (~4 GB) download inside the app on first run, pinned and SHA-256 verified.

Develop:

```powershell
packaging\dev.cmd sync --group dev
packaging\dev.cmd run pytest -q
packaging\dev.cmd run python -m lariksa.app
```

On Linux or WSL everything except the recorder and the window runs, which is enough for the tests and the pipeline:

```bash
uv sync
uv run pytest -q                          # 69 tests; the WASAPI test runs only on Windows
uv run python scripts/preview.py out/     # UI screenshots in headless Chrome with a stub API
```

Bench scripts in `scripts/`: `bench_asr.py` (Whisper settings side by side), `bench_notulen.py`, `bench_llm.py`, and `smoke.py` (the full pipeline through the service, no UI).

## Layout

| Module | Responsibility |
|---|---|
| `recorder.py`, `audio.py` | WASAPI two-track capture, 16 kHz mono, wall-clock padding, crash repair |
| `transcribe.py`, `hardware.py` | faster-whisper with Indonesian defaults; CUDA or CPU choice |
| `transcript.py` | Utterances, track merge, echo removal |
| `language.py` | Which language the notulen is written in |
| `notulen.py`, `llm.py` | Prompts, schema, map-reduce for long meetings; llama-server lifecycle and device choice |
| `pipeline.py` | Transcribe then summarise, with injected engines |
| `store.py` | SQLite + FTS5 search, settings, glossary |
| `service.py`, `bridge.py`, `app.py` | Worker queue, downloads, recovery; JavaScript API; window |
| `catalog.py` | Pinned, hash-verified, resumable downloads |
| `ui/` | Brand Kit v1.0 tokens, Plus Jakarta Sans, DM Mono |

## Privacy and consent

Audio, transcripts and notulen stay in the app folder. The network is used only to download models and runtimes. Recording other people may require their consent where you live; the app reminds you to tell participants.

## Licence

Code: [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for third-party attributions.

The Lariksa name, logo and icon are not covered by the code licence; see [TRADEMARKS.md](TRADEMARKS.md). Forks are welcome under their own name.

Third-party components, none of which are redistributed in this repository except the fonts:

- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) (MIT), [CTranslate2](https://github.com/OpenNMT/CTranslate2) (MIT), [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT), [pywebview](https://github.com/r0x0r/pywebview) (BSD-3-Clause), [PyAudioWPatch](https://github.com/s0d3s/PyAudioWPatch) (Apache-2.0), [python-docx](https://github.com/python-openxml/python-docx) (MIT), [PyAV](https://github.com/PyAV-Org/PyAV) (BSD-3-Clause)
- Models downloaded at first run: Whisper large-v3-turbo (MIT), Qwen3-4B-Instruct-2507 (Apache-2.0). NVIDIA cuBLAS and cuDNN are downloaded from PyPI under NVIDIA's licence.
- Whisper setup patterns adapted from [AutoClip](https://github.com/artbyjazi/autoclip) (MIT, Jad Ghazi); notice in [NOTICE](NOTICE).
- Fonts: Plus Jakarta Sans and DM Mono, SIL Open Font License 1.1 (`src/lariksa/ui/fonts/`).
