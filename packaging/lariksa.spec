# PyInstaller spec. Run from the repo root on Windows:
#   uv run --group dev pyinstaller packaging/lariksa.spec --noconfirm
#
# Builds both release flavours from one analysis:
#   Lariksa/            one-folder portable app (fast start; zipped for release)
#   Lariksa-onefile.exe single file (unpacks to %TEMP% on each start, so a few
#                       seconds slower to open)
# Either way, models, runtimes and meetings live next to the exe, never bundled.
from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs

ROOT = SPECPATH + "/.."

datas = [
    (ROOT + "/src/lariksa/ui", "lariksa/ui"),
    *collect_data_files("faster_whisper"),  # Silero VAD model (assets/*.onnx)
    *collect_data_files("webview"),
]
binaries = [*collect_dynamic_libs("ctranslate2"), *collect_dynamic_libs("onnxruntime")]

a = Analysis(
    [ROOT + "/packaging/launcher.py"],
    pathex=[ROOT + "/src"],
    datas=datas,
    binaries=binaries,
    hiddenimports=["webview.platforms.edgechromium", "pyaudiowpatch", "docx"],
    excludes=["tkinter", "matplotlib", "torch", "IPython", "pytest"],
    noarchive=False,
)
pyz = PYZ(a.pure)

common = dict(
    icon=ROOT + "/src/lariksa/ui/lariksa.ico",
    console=False,
    upx=False,
    version=ROOT + "/packaging/version.txt",
)

folder_exe = EXE(pyz, a.scripts, [], exclude_binaries=True, name="Lariksa", **common)
coll = COLLECT(folder_exe, a.binaries, a.datas, name="Lariksa", upx=False)

onefile_exe = EXE(pyz, a.scripts, a.binaries, a.datas, [], name="Lariksa-onefile",
                  runtime_tmpdir=None, **common)
