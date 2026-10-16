# Building Lariksa

Lariksa is a Rust workspace:

- `core/`: recording, transcription, notulen, storage and exports. No window, so it builds and tests on any OS.
- `desktop/`: the Tauri app for Windows (main window, recording pill, shortcut, taskbar progress).
- `ui/`: the HTML/CSS/JS both windows load.

The release is cross-compiled from Linux or WSL. Visual Studio is not needed.

## One-time setup (Linux / WSL)

```bash
# Rust and the Windows target
curl https://sh.rustup.rs -sSf | sh
rustup target add x86_64-pc-windows-msvc

# Windows SDK + CRT headers and libraries (accepting Microsoft's licence)
cargo install xwin --locked
xwin --accept-license splat --output ~/.local/xwin/sdk

# LLVM tools: clang-cl compiles SQLite, llvm-rc embeds the icon and version info
mkdir -p ~/.local/llvm && cd ~/.local/llvm
curl -LO https://github.com/llvm/llvm-project/releases/download/llvmorg-23.1.3/LLVM-23.1.3-Linux-X64.tar.xz
tar -xJf LLVM-23.1.3-Linux-X64.tar.xz
```

`scripts/xwin-env.sh` points Cargo at those folders. Override with `XWIN_SDK` and `LLVM_BIN` if you put them elsewhere.

## Everyday commands

```bash
cargo test -p lariksa-core              # all logic tests, on Linux
cargo clippy --workspace --all-targets  # lint (desktop needs the Windows target, see below)
packaging/build.sh                      # tests, Windows build, dist/Lariksa/ + dist/release/

source scripts/xwin-env.sh
cargo build --release -p lariksa-desktop --target x86_64-pc-windows-msvc
```

Run `dist/Lariksa/Lariksa.exe` on Windows. To test against a copy of your real meetings, copy `data/` (and `models/`, `runtime/` to skip the downloads) next to it.

## Testing on Windows

The Windows-only parts (WASAPI recording, the shortcut, the job object that stops child processes) have a test that runs only there:

```bash
source scripts/xwin-env.sh
cargo test -p lariksa-core --target x86_64-pc-windows-msvc --no-run   # then run the test exe from Windows
```

`core/examples/` has small Windows tools used during development: `probe` (record 3 s, list GPUs, transcribe a file), `bench` (whisper.cpp GPU vs CPU), `import` (import files and wait for the notulen), `export` (Markdown + DOCX of a meeting), `decode` (imported audio to 16 kHz WAV), `retranscribe` (print a recorded meeting's transcript as the app would make it now, without touching the database).

To drive the UI from a script, start the app with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333` and evaluate JavaScript over the DevTools protocol: every UI action is `window.pywebview.api.<name>(...)`.

## Data compatibility

`data/lariksa.db` keeps the schema and JSON shapes of the earlier Python version (0.1.x). An existing portable folder can be upgraded by replacing `Lariksa.exe` and `_internal/`; meetings, settings and the glossary carry over. Models move to whisper.cpp's format, so the app downloads the new transcription model (574 MB) and runtime (9 MB) once; the old `models/whisper-large-v3-turbo` (1.6 GB) and `runtime/cuda` (1.7 GB) folders can then be deleted.
