# Build the portable Lariksa folder on Windows.
#   powershell -ExecutionPolicy Bypass -File packaging\build.ps1
# Output: dist\Lariksa-portable.zip and dist\Lariksa\ (app files only).
#
# PyInstaller --noconfirm deletes its whole output folder, so it builds into
# build\pyi-dist and the result is copied over dist\Lariksa. That way a dev
# copy's models\, runtime\ and data\ (downloaded models, meetings) survive.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

$uv = ".tools\uv.exe"
if (-not (Test-Path $uv)) {
    New-Item -ItemType Directory -Force .tools | Out-Null
    Invoke-WebRequest https://github.com/astral-sh/uv/releases/download/0.12.24/uv-x86_64-pc-windows-msvc.zip -OutFile .tools\uv.zip
    Expand-Archive .tools\uv.zip .tools -Force
    Remove-Item .tools\uv.zip
}
$env:UV_PYTHON_INSTALL_DIR = "$PWD\.tools\python"
$env:UV_CACHE_DIR = "$PWD\.tools\cache"
$env:UV_PYTHON_PREFERENCE = "only-managed"

& $uv sync --group dev --python 3.12
& $uv run pytest -q -p no:cacheprovider
if ($LASTEXITCODE -ne 0) { throw "Tests failed" }
& $uv run pyinstaller packaging/lariksa.spec --noconfirm --distpath build\pyi-dist --workpath build\pyi-work --log-level WARN
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed" }

$app = "dist\Lariksa"
New-Item -ItemType Directory -Force $app | Out-Null
if (Test-Path "$app\_internal") { Remove-Item -Recurse -Force "$app\_internal" }
Copy-Item -Recurse -Force build\pyi-dist\Lariksa\* $app
Copy-Item -Force packaging\BACA-SAYA.txt $app
foreach ($f in 'LICENSE', 'NOTICE', 'TRADEMARKS.md') { Copy-Item -Force $f $app; Copy-Item -Force $f build\pyi-dist\Lariksa }

# The zip carries the app only; models download on first run.
$zip = "dist\Lariksa-portable.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path build\pyi-dist\Lariksa, packaging\BACA-SAYA.txt -DestinationPath $zip
Write-Host "Done: $app\Lariksa.exe and $zip"
