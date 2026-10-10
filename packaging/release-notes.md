Lariksa 0.2.0 is the same app, rebuilt in Rust (Tauri) to be much lighter. Every feature of 0.1.0 is still there: two-track recording, the floating indicator and Ctrl+Shift+R, notulen with chapters, owners and timestamps, inline editing, WhatsApp copy, Word and Markdown export, import of recordings and Read.ai/Meet/Zoom transcripts, search, and crash recovery.

## Download

| File | Size | Use it when |
|---|---|---|
| **Lariksa-portable.zip** | 4 MB | Recommended. Extract anywhere and run `Lariksa\Lariksa.exe`. |
| **Lariksa.exe** | 8 MB | The same app as a single file. Put it in its own folder first: models and meetings are saved next to it. |

On first run, click **Unduh semua** to download the AI models once (3.1 GB). After that Lariksa works offline.

**Requirements:** Windows 10 2004+ or Windows 11 (x64) with WebView2 (built in on Windows 11 and current Windows 10), 8 GB RAM (16 GB recommended), about 4 GB of free disk. A graphics card is optional and makes transcription about 17x faster.

**"Windows protected your PC"?** The app is not code-signed yet. Choose **More info → Run anyway**. You can check the file against `SHA256SUMS.txt`:

```powershell
Get-FileHash .\Lariksa-portable.zip -Algorithm SHA256
```

## What changed from 0.1.0

| | 0.1.0 (Python) | 0.2.0 (Rust) |
|---|---|---|
| Download | 96 MB | 4 MB |
| App files | 250 MB | 8 MB |
| App memory | 138 MB | 30 MB (+ Windows' WebView2) |
| Opens in | a few seconds | 0.1 s |
| AI downloads | 4 GB (+1.3 GB NVIDIA) | 3.1 GB |
| Transcribing 10 min on GPU | 21 s, NVIDIA only | 36 s, NVIDIA, AMD or Intel |

- Transcription now uses whisper.cpp instead of faster-whisper. The wording of transcripts differs slightly from 0.1.0 (about 80% of words match on the same audio; neither is clearly more accurate), so regenerated notulen can differ a little too.
- The NVIDIA download (cuBLAS and cuDNN, 1.3 GB) is gone: both engines use the GPU through Vulkan and pick the discrete GPU over the built-in one.

## Upgrading from 0.1.0

Replace `Lariksa.exe` in your existing folder and delete `_internal\`. Your meetings, settings and glossary carry over. Open Lariksa and click **Unduh semua** once to fetch the new transcription files (583 MB). You can then delete `models\whisper-large-v3-turbo\` and `runtime\cuda\` to free 3.3 GB.

## Known limitations

- Tested on one laptop (Windows 11, RTX 3050 Ti) with audio played through the speakers; not yet on Windows 10, on a laptop with only built-in graphics, or in a live Zoom/Meet call.
- Without any usable graphics card, transcribing 10 minutes takes about 10 minutes on an i7-12650H (0.1.0 took about 3). Pick **Whisper small** in Settings for about 2 minutes, at lower accuracy.
- Imported video with Dolby Digital (AC-3) sound, common on DVDs and TV recordings, needs Windows' AC-3 codec, which Windows 11 24H2 no longer includes. Zoom, Teams and Meet recordings (AAC, Opus) are not affected.
- On laptop speakers your mic also hears the other participants. Lariksa removes that echo, but a headset gives the cleanest transcript.
