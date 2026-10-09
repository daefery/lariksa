First public build of Lariksa: portable meeting notes for Windows. Record any Meet, Zoom or Teams call; when you stop, Lariksa writes the transcript and the notulen (summary, decisions, action items with owner and deadline, open questions) with AI that runs on your own laptop.

## Download

| File | Size | Use it when |
|---|---|---|
| **Lariksa-portable.zip** | 96 MB | Recommended. Extract anywhere and run `Lariksa\Lariksa.exe`. Opens fastest. |
| **Lariksa.exe** | 96 MB | You want a single file. Put it in its own folder first: models and meetings are saved next to it. Opens a few seconds slower. |

On first run, click **Unduh semua** to download the AI models once (about 4 GB, or 5.3 GB with the optional NVIDIA GPU files). After that Lariksa works offline.

**Requirements:** Windows 10 2004+ or Windows 11 (x64), 8 GB RAM (16 GB recommended), about 6 GB of free disk. An NVIDIA GPU is optional and makes transcription about 9x faster.

**"Windows protected your PC"?** The app is not code-signed yet. Choose **More info → Run anyway**. You can check the file against `SHA256SUMS.txt`:

```powershell
Get-FileHash .\Lariksa-portable.zip -Algorithm SHA256
```

## What's in 0.1.0

- Two-track recording: your mic ("Saya") and the meeting audio ("Peserta"), no bot in the call
- Transcription with Whisper large-v3-turbo, tuned for Indonesian with English work terms
- Notulen from a local Qwen3-4B model, in the language of the meeting (Indonesian or English)
- Glossary for names and jargon, inline editing, search across meetings
- Copy for WhatsApp, export to DOCX and Markdown
- Recordings survive crashes; processing resumes on the next start

## Known limitations

- Tested on one laptop (Windows 11, RTX 3050 Ti) with audio played through the speakers; not yet on Windows 10, on a laptop without an NVIDIA GPU, or in a live Zoom/Meet call.
- On laptop speakers your mic also hears the other participants. Lariksa removes that echo, but a headset gives the cleanest transcript.
- Language detection for the notulen is a word count; pin a language in Settings if a meeting comes out in the wrong one.
