# Build Lariksa on Windows.
#   powershell -ExecutionPolicy Bypass -File packaging\build.ps1
#
# Output in dist\:
#   Lariksa\                 the portable app folder (a dev copy keeps its models\, runtime\, data\)
#   release\Lariksa-portable.zip   app folder, zipped
#   release\Lariksa.exe            single-file app
#   release\SHA256SUMS.txt
#
# PyInstaller --noconfirm deletes its whole output folder, so it builds into
# build\pyi-dist and the result is copied over dist\Lariksa. That way a dev
# copy's downloaded models and meetings survive a rebuild.
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
# --clean: PyInstaller caches each EXE's icon and version resources in the work
# folder; without it a changed lariksa.ico can silently keep the old icon.
& $uv run pyinstaller packaging/lariksa.spec --noconfirm --clean --distpath build\pyi-dist --workpath build\pyi-work --log-level WARN
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed" }

$docs = 'LICENSE', 'NOTICE', 'TRADEMARKS.md', 'packaging\BACA-SAYA.txt'
$pyi = "build\pyi-dist\Lariksa"
foreach ($f in $docs) { Copy-Item -Force $f $pyi }

# Dev copy: refresh app files only.
$app = "dist\Lariksa"
New-Item -ItemType Directory -Force $app | Out-Null
if (Test-Path "$app\_internal") { Remove-Item -Recurse -Force "$app\_internal" }
Copy-Item -Recurse -Force "$pyi\*" $app

# Release assets. Models download on first run, so both stay small.
$rel = "dist\release"
if (Test-Path $rel) { Remove-Item -Recurse -Force $rel }
New-Item -ItemType Directory -Force $rel | Out-Null
# tar.exe (built into Windows 10 1803+) writes a standard zip; Windows PowerShell's
# Compress-Archive stores backslash paths that many unzip tools mis-extract.
& tar.exe -a -c -f "$rel\Lariksa-portable.zip" -C build\pyi-dist Lariksa
if ($LASTEXITCODE -ne 0) { throw "zip failed" }
Copy-Item -Force build\pyi-dist\Lariksa-onefile.exe "$rel\Lariksa.exe"
Get-ChildItem $rel -File | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content -Encoding ascii "$rel\SHA256SUMS.txt"

Get-ChildItem $rel | Format-Table Name, @{n = "MB"; e = { [math]::Round($_.Length / 1MB, 1) } }
Write-Host "Done: $app\Lariksa.exe and $rel\"
