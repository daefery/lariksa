@echo off
rem Run a uv command with a self-contained toolchain in .tools\ (no system Python needed).
rem   packaging\dev.cmd sync --group dev
rem   packaging\dev.cmd run pytest -q
setlocal
cd /d "%~dp0.."
set "UV_PYTHON_INSTALL_DIR=%CD%\.tools\python"
set "UV_CACHE_DIR=%CD%\.tools\cache"
set "UV_PYTHON_PREFERENCE=only-managed"
"%CD%\.tools\uv.exe" %*
