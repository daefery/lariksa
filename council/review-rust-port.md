# Review brief: Lariksa Python -> Rust (Tauri) port

Repo: /home/feryyp/code/lariksa, branch `rust-tauri`, commit dc7bebf. Compare against `9af7cdb` (the Python app).

The user's requirement: "move to Rust (Tauri), make sure nothing on the feature changed". They accepted one known change: transcription moved from faster-whisper to whisper.cpp, so transcript wording differs slightly.

Review with the world-class-reviewer skill. Focus on:
1. Feature parity: any Python behaviour (service.py, pipeline.py, store.py, minutes_writer.py, recorder.py, overlay.py, bridge.py, media.py, export.py, catalog.py) that the Rust port in core/src and desktop/src dropped or changed. `git show 9af7cdb:src/lariksa/<file>` shows the old code.
2. Correctness bugs: concurrency (service.rs worker, recorder.rs threads, locks), error paths, data loss (recording stop/crash, store updates), Windows-only code (recorder.rs wasapi_impl, hotkey.rs, process.rs job object, desktop clipboard).
3. Security: media.rs token server, llm.rs API key, file paths from the UI.

Already verified by running the real app on Windows: existing meetings load with identical JSON, Markdown export byte-identical, DOCX same paragraphs/table, recording + hotkey + pill (no focus steal), crash recovery, imports (webm, m4a, txt), first-run download, GPU selection, full notulen run.

Keep the answer SHORT: at most 12 findings, each one line: [blocker|major|minor] file:line - problem - fix. Then a one-line verdict. Write it to council/review-rust-port.answer.md and reply only with that path.
