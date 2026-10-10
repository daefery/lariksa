#!/usr/bin/env bash
# Build the Windows release from Linux or WSL (no Visual Studio needed).
#
#   packaging/build.sh
#
# Output in dist/:
#   Lariksa/                       the portable app folder
#   release/Lariksa-portable.zip   that folder, zipped
#   release/Lariksa.exe            the app on its own (a single file)
#   release/SHA256SUMS.txt
#
# One-time setup (see docs/BUILD.md): rustup target x86_64-pc-windows-msvc,
# xwin's Windows SDK in ~/.local/xwin/sdk, LLVM's clang-cl/llvm-lib/llvm-rc.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test -p lariksa-core --quiet
source scripts/xwin-env.sh
cargo build --release -p lariksa-desktop --target x86_64-pc-windows-msvc

exe=target/x86_64-pc-windows-msvc/release/lariksa.exe
app=dist/Lariksa
mkdir -p "$app"
# A dev copy keeps its models/, runtime/ and data/; only app files are refreshed.
cp "$exe" "$app/Lariksa.exe"
cp LICENSE NOTICE TRADEMARKS.md packaging/BACA-SAYA.txt "$app/"

rel=dist/release
rm -rf "$rel" && mkdir -p "$rel" dist/stage/Lariksa
cp "$app/Lariksa.exe" "$app"/{LICENSE,NOTICE,TRADEMARKS.md,BACA-SAYA.txt} dist/stage/Lariksa/
(cd dist/stage && zip -qr9 "../release/Lariksa-portable.zip" Lariksa)
rm -rf dist/stage
cp "$app/Lariksa.exe" "$rel/Lariksa.exe"
(cd "$rel" && sha256sum Lariksa-portable.zip Lariksa.exe > SHA256SUMS.txt)
ls -la "$rel"
