"""Pick how Whisper runs on this machine.

Adapted from AutoClip (MIT, Jad Ghazi): ask CTranslate2 what the device really
supports instead of guessing from the GPU name, and make the NVIDIA DLLs that
were downloaded next to the app findable by the loader. Without that
registration the model loads fine and then dies at the first inference with
``Library cublas64_12.dll is not found``.
"""

from __future__ import annotations

import logging
import os
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

log = logging.getLogger(__name__)

#: Preferred compute types, best first. int8 weights keep the turbo model
#: under ~1 GB of VRAM, which matters on 4 GB laptop GPUs.
_GPU_ORDER = ("int8_float16", "int8_bfloat16", "float16", "int8", "float32")
_CPU_ORDER = ("int8", "int8_float32", "float32")


@dataclass(frozen=True)
class Compute:
    device: str
    compute_type: str
    gpu_name: str = ""

    @property
    def label(self) -> str:
        if self.device == "cuda":
            return f"GPU ({self.gpu_name})"
        return "CPU (GPU NVIDIA terdeteksi)" if self.gpu_name else "CPU"

    @property
    def gpu_available(self) -> bool:
        """An NVIDIA GPU is present, whether or not its libraries are installed yet."""
        return bool(self.gpu_name)


def register_cuda_libraries(cuda_dir: Path) -> bool:
    """Make downloaded cuBLAS/cuDNN DLLs loadable. Safe to call when absent."""
    if sys.platform != "win32" or not (cuda_dir / "cublas64_12.dll").exists():
        return False
    cuda_dir = cuda_dir.resolve()  # add_dll_directory rejects relative paths (WinError 87)
    os.add_dll_directory(str(cuda_dir))
    # Some backends load by bare name, which add_dll_directory does not cover.
    os.environ["PATH"] = f"{cuda_dir}{os.pathsep}{os.environ.get('PATH', '')}"
    return True


def nvidia_gpu_name() -> str:
    """Name of the first NVIDIA GPU, or "" when there is none (or no driver)."""
    try:
        import ctranslate2

        if ctranslate2.get_cuda_device_count() < 1:
            return ""
    except Exception:  # pragma: no cover - platform dependent
        return ""
    try:  # nvidia-smi ships with the driver; the name is only for display
        import subprocess

        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=name", "--format=csv,noheader"],
            capture_output=True, text=True, timeout=5,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        return out.stdout.strip().splitlines()[0] if out.returncode == 0 else "NVIDIA"
    except Exception:
        return "NVIDIA"


def _pick(supported: set[str], order: tuple[str, ...]) -> str:
    return next((c for c in order if c in supported), order[-1])


@lru_cache(maxsize=1)
def detect(cuda_ready: bool) -> Compute:
    """Best available compute. ``cuda_ready`` means the CUDA DLLs can be loaded."""
    import ctranslate2

    gpu = nvidia_gpu_name()
    if gpu and cuda_ready:
        try:
            supported = set(ctranslate2.get_supported_compute_types("cuda"))
            return Compute("cuda", _pick(supported, _GPU_ORDER), gpu)
        except Exception as exc:  # pragma: no cover - platform dependent
            log.warning("CUDA present but unusable (%s); using CPU.", exc)
    supported = set(ctranslate2.get_supported_compute_types("cpu"))
    # Keep the GPU name on CPU fallback: the UI uses it to offer the CUDA download.
    return Compute("cpu", _pick(supported, _CPU_ORDER), gpu)
