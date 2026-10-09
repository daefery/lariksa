# Lariksa

Portable meeting notes for Windows that never leave your laptop. Press **Rekam** in any Meet, Zoom or Teams call; when you hang up, Lariksa writes the transcript and notulen (summary, decisions, action items with PIC and deadline, open questions), ready to paste into WhatsApp or export to DOCX.

No bot joins the call, there is no subscription, and nothing is uploaded.

## How it works

```
mic  ──► mic.wav    ──┐                    ┌─► notulen (JSON schema) ─► UI / DOCX / MD / WhatsApp
                      ├─► faster-whisper ──┤
loopback ► system.wav ┘   (turbo, id, VAD) └─► transcript "Saya" / "Peserta"
             WASAPI            CUDA or CPU            llama.cpp + Qwen3-4B (Vulkan dGPU, iGPU or CPU)
```

- **Two tracks, free speaker labels.** The mic is you ("Saya"), the system loopback is everyone else ("Peserta"). The mic's echo of the speakers is dropped by overlap + similarity, so no diarization model is needed.
- **Indonesian-first.** Language is pinned (`id`) instead of guessed from the first 30 s; a glossary of names and jargon goes in as `hotwords` and `initial_prompt`.
- **One heavy job at a time.** Whisper is unloaded before the LLM starts, so 8 GB laptops cope.
- **Portable.** Everything (models, runtime, `data/` with meetings) lives next to `Lariksa.exe`. Copy the folder, and your meetings come with it.

## Measured (RTX 3050 Ti laptop, i7-12650H, 10 min of real Indonesian podcast audio)

| Step | GPU | CPU only |
|---|---|---|
| Transcription, Whisper large-v3-turbo | 21 s (~28x real time) | 198 s (~3x real time) |
| Notulen, Qwen3-4B Q4_K_M | 48 s | ~200 s |

A 1-hour meeting is roughly 2 min transcription + 2-3 min notulen on this GPU (extrapolated, not yet measured).

## Develop

```bash
uv sync                 # Linux/WSL: everything except the recorder and window
uv run pytest -q        # 54 tests; the WASAPI test runs only on Windows
uv run python scripts/preview.py out/   # UI screenshots in headless Chrome with a stub API
```

Windows (no system Python needed; uv and Python live in `.tools/`):

```powershell
packaging\dev.cmd sync --group dev
packaging\dev.cmd run pytest -q
packaging\dev.cmd run python -m lariksa.app
powershell -ExecutionPolicy Bypass -File packaging\build.ps1   # -> dist\Lariksa-portable.zip
```

Bench scripts: `scripts/bench_asr.py` (Whisper settings side by side), `scripts/bench_notulen.py`, `scripts/bench_llm.py`, `scripts/smoke.py` (full pipeline through the service, no UI).

## Layout

| Module | Responsibility |
|---|---|
| `recorder.py`, `audio.py` | WASAPI two-track capture, 16 kHz mono, wall-clock padding, crash repair |
| `transcribe.py`, `hardware.py` | faster-whisper with Indonesian defaults; CUDA/CPU choice |
| `transcript.py` | Utterances, track merge, echo removal |
| `notulen.py`, `llm.py` | Prompt, schema, map-reduce for long meetings; llama-server lifecycle and device choice |
| `pipeline.py` | Transcribe then summarise, with injected engines |
| `store.py` | SQLite + FTS5 search, settings, glossary |
| `service.py`, `bridge.py`, `app.py` | Worker queue, downloads, recovery; JS API; window |
| `catalog.py` | Pinned, hash-verified, resumable downloads |
| `ui/` | Brand Kit v1.0 tokens, Plus Jakarta Sans, DM Mono |

## Credits and licences

- Whisper setup patterns (compute-type detection, CUDA DLL registration) adapted from [AutoClip](https://github.com/artbyjazi/autoclip), MIT, Jad Ghazi.
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) (MIT), [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT), [pywebview](https://github.com/r0x0r/pywebview) (BSD), [PyAudioWPatch](https://github.com/s0d3s/PyAudioWPatch) (MIT).
- Models, downloaded at first run: Whisper large-v3-turbo (MIT), Qwen3-4B-Instruct-2507 (Apache-2.0). NVIDIA cuBLAS/cuDNN are downloaded from PyPI under NVIDIA's licence, not redistributed.
- Fonts: Plus Jakarta Sans and DM Mono, SIL Open Font License (`src/lariksa/ui/fonts/`).

The Lariksa name and mark are proposed and not yet trademark-cleared; see `Lariksa_Brand_Kit_v1.0/08_Legal_and_Handoff/`.
