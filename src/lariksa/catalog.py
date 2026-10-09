"""Downloadable assets: models and native runtimes, pinned and verified.

The app ships small; the heavy parts are fetched on first run into the
portable folder. Every URL is pinned to an exact revision and every large file
to a SHA-256, so a moved or tampered upstream file fails loudly instead of
loading silently.
"""

from __future__ import annotations

import hashlib
import shutil
import sys
import tarfile
import zipfile
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

import httpx

from .paths import Paths

Progress = Callable[[int, int], None]

_HF = "https://huggingface.co"
_LLAMA_TAG = "b11514"
_LLAMA = f"https://github.com/ggml-org/llama.cpp/releases/download/{_LLAMA_TAG}"
_PYPI = "https://files.pythonhosted.org/packages"


@dataclass(frozen=True)
class RemoteFile:
    url: str
    name: str
    size: int
    sha256: str = ""


@dataclass(frozen=True)
class Asset:
    key: str
    label: str
    folder: str
    files: tuple[RemoteFile, ...]
    #: "files" are used as-is; "zip"/"tar" archives are unpacked into the folder.
    unpack: str = "files"
    #: For archives: keep only members whose path contains one of these parts.
    keep: tuple[str, ...] = field(default_factory=tuple)
    #: A file that proves the asset is installed, relative to its folder.
    marker: str = ""

    @property
    def size(self) -> int:
        return sum(f.size for f in self.files)

    def path(self, paths: Paths) -> Path:
        base = paths.runtime if self.folder.startswith("runtime/") else paths.models
        return base / self.folder.split("/", 1)[1]

    def installed(self, paths: Paths) -> bool:
        return (self.path(paths) / self.marker).exists()


def _hf(repo: str, rev: str, name: str, size: int, sha: str = "") -> RemoteFile:
    return RemoteFile(f"{_HF}/{repo}/resolve/{rev}/{name}", name, size, sha)


_TURBO = ("dropbox-dash/faster-whisper-large-v3-turbo", "0a363e9161cbc7ed1431c9597a8ceaf0c4f78fcf")
_SMALL = ("Systran/faster-whisper-small", "536b0662742c02347bc0e980a01041f333bce120")
_QWEN = ("unsloth/Qwen3-4B-Instruct-2507-GGUF", "a06e946bb6b655725eafa393f4a9745d460374c9")

WHISPER_TURBO = Asset(
    key="whisper-turbo",
    label="Model transkripsi (Whisper large-v3-turbo)",
    folder="models/whisper-large-v3-turbo",
    marker="model.bin",
    files=(
        _hf(*_TURBO, "config.json", 2263),
        _hf(*_TURBO, "preprocessor_config.json", 340),
        _hf(*_TURBO, "tokenizer.json", 2710337),
        _hf(*_TURBO, "vocabulary.json", 1068114),
        _hf(*_TURBO, "model.bin", 1617884929,
            "e76620f83d5f5b69efd3d87e3dc180c1bd21df9fbebacfd4335e5e1efcc018da"),
    ),
)

WHISPER_SMALL = Asset(
    key="whisper-small",
    label="Model transkripsi ringan (Whisper small)",
    folder="models/whisper-small",
    marker="model.bin",
    files=(
        _hf(*_SMALL, "config.json", 2370),
        _hf(*_SMALL, "tokenizer.json", 2203239),
        _hf(*_SMALL, "vocabulary.txt", 459861),
        _hf(*_SMALL, "model.bin", 483546902,
            "3e305921506d8872816023e4c273e75d2419fb89b24da97b4fe7bce14170d671"),
    ),
)

SUMMARY_MODEL = Asset(
    key="llm",
    label="Model notulen (Qwen3 4B Instruct)",
    folder="models/llm",
    marker="Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
    files=(
        _hf(*_QWEN, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf", 2497281120,
            "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597"),
    ),
)

# Vulkan runs on NVIDIA, AMD and Intel graphics, and the build carries the CPU
# backend too, so one download covers every Windows laptop.
_LLAMA_WIN = RemoteFile(
    f"{_LLAMA}/llama-{_LLAMA_TAG}-bin-win-vulkan-x64.zip",
    "llama-win-vulkan-x64.zip",
    33446009,
    "e1eaddcde761b1f839b9c6d5924b97f2698fba22068ca3b63f129c3caf55b88f",
)
_LLAMA_LINUX = RemoteFile(
    f"{_LLAMA}/llama-{_LLAMA_TAG}-bin-ubuntu-x64.tar.gz",
    "llama-ubuntu-x64.tar.gz",
    17793295,
    "b2b617fb28e9d1444235c30bb09a3704789a1840e16ead58a5f7be6eeae8beef",
)
LLAMA_SERVER_NAME = "llama-server.exe" if sys.platform == "win32" else "llama-server"

LLAMA_RUNTIME = Asset(
    key="llama",
    label="Mesin AI lokal (llama.cpp)",
    folder="runtime/llama",
    files=(_LLAMA_WIN if sys.platform == "win32" else _LLAMA_LINUX,),
    unpack="zip" if sys.platform == "win32" else "tar",
    marker=LLAMA_SERVER_NAME,
)

# faster-whisper's CTranslate2 needs cuBLAS 12 and cuDNN 9 for NVIDIA GPUs.
# They are published as wheels (zip files); only the DLLs are kept.
CUDA_RUNTIME = Asset(
    key="cuda",
    label="Akselerasi GPU NVIDIA (cuBLAS + cuDNN)",
    folder="runtime/cuda",
    unpack="zip",
    keep=(".dll",),
    marker="cublas64_12.dll",
    files=(
        RemoteFile(
            f"{_PYPI}/70/61/7d7b3c70186fb651d0fbd35b01dbfc8e755f69fd58f817f3d0f642df20c3/"
            "nvidia_cublas_cu12-12.8.4.1-py3-none-win_amd64.whl",
            "nvidia_cublas.whl", 567544208,
            "47e9b82132fa8d2b4944e708049229601448aaad7e6f296f630f2d1a32de35af",
        ),
        RemoteFile(
            f"{_PYPI}/3d/90/0bd6e586701b3a890fd38aa71c387dab4883d619d6e5ad912ccbd05bfd67/"
            "nvidia_cudnn_cu12-9.10.2.21-py3-none-win_amd64.whl",
            "nvidia_cudnn.whl", 692992268,
            "c6288de7d63e6cf62988f0923f96dc339cea362decb1bf5b3141883392a7d65e",
        ),
    ),
)

ASSETS = {a.key: a for a in (WHISPER_TURBO, WHISPER_SMALL, SUMMARY_MODEL, LLAMA_RUNTIME, CUDA_RUNTIME)}


class DownloadError(RuntimeError):
    """An asset could not be fetched or failed verification."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def _fetch(remote: RemoteFile, target: Path, done_before: int, total: int,
           progress: Progress | None, cancelled: Callable[[], bool]) -> None:
    """Download one file with resume support, then verify its hash."""
    partial = target.with_name(target.name + ".part")
    offset = partial.stat().st_size if partial.exists() else 0
    headers = {"Range": f"bytes={offset}-"} if offset else {}

    with httpx.stream("GET", remote.url, headers=headers, follow_redirects=True,
                      timeout=httpx.Timeout(30.0, read=120.0)) as response:
        if response.status_code == 416:  # already complete
            pass
        elif response.status_code not in (200, 206):
            raise DownloadError(f"{remote.name}: HTTP {response.status_code}")
        else:
            if response.status_code == 200:
                offset = 0  # server ignored the range; start over
            with partial.open("ab" if offset else "wb") as out:
                written = offset
                for chunk in response.iter_bytes(1 << 20):
                    if cancelled():
                        raise DownloadError("Unduhan dibatalkan.")
                    out.write(chunk)
                    written += len(chunk)
                    if progress:
                        progress(done_before + written, total)

    if remote.sha256 and _sha256(partial) != remote.sha256:
        partial.unlink(missing_ok=True)
        raise DownloadError(f"{remote.name}: file rusak (hash tidak cocok). Coba unduh lagi.")
    partial.replace(target)


def _unpack(archive: Path, dest: Path, kind: str, keep: tuple[str, ...]) -> None:
    """Extract an archive flat into ``dest`` (the runtime's own folder layout is not needed)."""
    def wanted(name: str) -> bool:
        return not keep or any(part in name for part in keep)

    if kind == "zip":
        with zipfile.ZipFile(archive) as zf:
            for member in zf.infolist():
                if member.is_dir() or not wanted(member.filename):
                    continue
                with zf.open(member) as src, (dest / Path(member.filename).name).open("wb") as out:
                    shutil.copyfileobj(src, out)
    else:
        with tarfile.open(archive) as tf:
            for member in tf.getmembers():
                if not (member.isfile() or member.issym()) or not wanted(member.name):
                    continue
                target = dest / Path(member.name).name
                if member.issym():
                    target.unlink(missing_ok=True)
                    target.symlink_to(Path(member.linkname).name)
                    continue
                src = tf.extractfile(member)
                if src is None:
                    continue
                with src, target.open("wb") as out:
                    shutil.copyfileobj(src, out)
                target.chmod(member.mode | 0o200)


def install(asset: Asset, paths: Paths, progress: Progress | None = None,
            cancelled: Callable[[], bool] = lambda: False) -> Path:
    """Download (resuming if interrupted), verify and unpack an asset."""
    dest = asset.path(paths)
    if asset.installed(paths):
        return dest
    dest.mkdir(parents=True, exist_ok=True)
    staging = paths.root / "downloads"
    staging.mkdir(exist_ok=True)

    done = 0
    for remote in asset.files:
        target = (dest if asset.unpack == "files" else staging) / remote.name
        if not target.exists():
            _fetch(remote, target, done, asset.size, progress, cancelled)
        done += remote.size
        if asset.unpack != "files":
            _unpack(target, dest, asset.unpack, asset.keep)
            target.unlink(missing_ok=True)

    if not asset.installed(paths):
        raise DownloadError(f"{asset.label}: unduhan selesai tapi {asset.marker} tidak ditemukan.")
    return dest
