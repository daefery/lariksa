Lariksa 0.3.0 can write the notulen through an API instead of the local model. Recording and transcription still run on your laptop, and the local model stays the default.

## Download

| File | Size | Use it when |
|---|---|---|
| **Lariksa-portable.zip** | 4 MB | Recommended. Extract anywhere and run `Lariksa\Lariksa.exe`. |
| **Lariksa.exe** | 8 MB | The same app as a single file. Put it in its own folder first: models and meetings are saved next to it. |

On first run, click **Unduh semua** to download the AI models once (3.1 GB, or 0.6 GB if you will use an API for the notulen). After that recording and transcription work offline.

**Requirements:** Windows 10 2004+ or Windows 11 (x64) with WebView2 (built in on Windows 11 and current Windows 10), 8 GB RAM (16 GB recommended), about 4 GB of free disk. A graphics card is optional and makes transcription about 17x faster.

**"Windows protected your PC"?** The app is not code-signed yet. Choose **More info → Run anyway**. You can check the file against `SHA256SUMS.txt`:

```powershell
Get-FileHash .\Lariksa-portable.zip -Algorithm SHA256
```

## What's new

- **Notulen through an API (optional).** Settings → Who writes the minutes → **An OpenAI-compatible API**. Enter the address (for example `https://api.openai.com/v1`, or a gateway such as CLIProxyAPI at `http://127.0.0.1:8317/v1`), the key and the model, then **Test connection** to list the models the API offers.
  - Only the transcript text is sent to that address. Audio never leaves the laptop.
  - The key is never shown again after saving. Changing the address without entering a new key clears the old one, so a key is never sent to a different server.
  - The 2.5 GB local notulen model is then not needed: **Remove** it under Models & engines, and download it again any time.
- **More action items, including your own.** On a 12-minute standup tested with an API model, the notulen went from 7 action items for 3 people to 27–29 items covering all 7 people who gave an update, including the recorder's own tasks ("Saya"), which were dropped before. The local model is unchanged at up to 10 items per part of the transcript.
- **No more "refs: []" in the text.** Line references a model copied into highlights or summaries are removed.
- **Steadier API runs.** A call that stalls is stopped after 150 s and sent once more, as is one the server rejects as busy (429) or broken (5xx).
- **Clearer log.** `data\lariksa.log` now records every notulen step that failed and why, how many items each part of the transcript produced and kept, and which action items were dropped.

## Local model or API?

Tested on one 12-minute Indonesian standup, same transcript:

| | Local (Qwen3 4B, RTX 3050 Ti) | API (`gpt-6-astra` through CLIProxyAPI) |
|---|---|---|
| Time for the notulen | 91 s | 106–126 s |
| Action items | 8, owners shown as line numbers | 27–29, with names (a few vague, such as "Semua peserta") |
| Who said what | mixed up 3 times | correct |
| Chapters | stopped at minute 4 | whole meeting |
| Data sent | nothing | transcript text |

API speed depends on the provider: through the same gateway one call took over 5 minutes, which is why stalled calls are now retried.

## Upgrading from 0.2.0

Replace `Lariksa.exe` in your existing folder. Your meetings, settings and models carry over, and nothing new needs to be downloaded.

## Known limitations

- API mode was tested with one gateway (CLIProxyAPI) and three models from it (`gpt-6-astra`, `gemini-3-flash`, `claude-sonnet-4-6`), not directly against OpenAI.
- Transcription still runs only on the laptop; the gateway tested has no transcription endpoint.
- Tested on one laptop (Windows 11, RTX 3050 Ti) with audio played through the speakers; not yet on Windows 10, on a laptop with only built-in graphics, or in a live Zoom/Meet call.
- Imported video with Dolby Digital (AC-3) sound, common on DVDs and TV recordings, needs Windows' AC-3 codec, which Windows 11 24H2 no longer includes. Zoom, Teams and Meet recordings (AAC, Opus) are not affected.
- On laptop speakers your mic also hears the other participants. Lariksa removes that echo, but a headset gives the cleanest transcript.
