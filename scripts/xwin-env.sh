# Cross-compile for Windows from Linux/WSL without Visual Studio:
#   source scripts/xwin-env.sh && cargo build --release --target x86_64-pc-windows-msvc
# Needs the Windows SDK + CRT unpacked by xwin (~/.local/xwin/sdk) and LLVM tools
# (clang-cl, llvm-lib, llvm-rc) on PATH; see docs/BUILD.md.
X="${XWIN_SDK:-$HOME/.local/xwin/sdk}"
L="${LLVM_BIN:-$HOME/.local/llvm/LLVM-23.1.3-Linux-X64/bin}"
export PATH="$L:$PATH"
export CC_x86_64_pc_windows_msvc=clang-cl CXX_x86_64_pc_windows_msvc=clang-cl AR_x86_64_pc_windows_msvc=llvm-lib
_inc="/imsvc$X/crt/include /imsvc$X/sdk/include/ucrt /imsvc$X/sdk/include/um /imsvc$X/sdk/include/shared"
export CFLAGS_x86_64_pc_windows_msvc="--target=x86_64-pc-windows-msvc $_inc"
export CXXFLAGS_x86_64_pc_windows_msvc="$CFLAGS_x86_64_pc_windows_msvc"
export INCLUDE="$X/crt/include;$X/sdk/include/ucrt;$X/sdk/include/um;$X/sdk/include/shared"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=rust-lld
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Lnative=$X/crt/lib/x86_64 -Lnative=$X/sdk/lib/um/x86_64 -Lnative=$X/sdk/lib/ucrt/x86_64"
