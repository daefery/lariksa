# PyInstaller spec: one-folder portable build. Run from the repo root on Windows:
#   uv run --group dev pyinstaller packaging/lariksa.spec --noconfirm
# Models and llama.cpp are NOT bundled; the app downloads them into its folder.
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
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="Lariksa",
    icon=ROOT + "/src/lariksa/ui/lariksa.ico",
    console=False,
    upx=False,
    version=ROOT + "/packaging/version.txt",
)
coll = COLLECT(exe, a.binaries, a.datas, name="Lariksa", upx=False)
