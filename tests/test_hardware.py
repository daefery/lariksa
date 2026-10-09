from lariksa import hardware
from lariksa.hardware import Compute


def test_cpu_fallback_keeps_gpu_name(monkeypatch):
    """Without CUDA libraries the app runs on CPU but must still know a GPU exists,
    otherwise it never offers the download that makes transcription 9x faster."""
    hardware.detect.cache_clear()
    monkeypatch.setattr(hardware, "nvidia_gpu_name", lambda: "RTX 3050 Ti")
    compute = hardware.detect(False)
    hardware.detect.cache_clear()
    assert compute.device == "cpu"
    assert compute.gpu_available
    assert compute.label == "CPU (GPU NVIDIA terdeteksi)"


def test_labels():
    assert Compute("cuda", "int8_float16", "RTX 3050 Ti").label == "GPU (RTX 3050 Ti)"
    assert Compute("cpu", "int8").label == "CPU"
    assert not Compute("cpu", "int8").gpu_available


def test_pick_prefers_int8_float16_on_gpu():
    assert hardware._pick({"float32", "int8_float16", "float16"}, hardware._GPU_ORDER) == "int8_float16"
    assert hardware._pick({"float32"}, hardware._CPU_ORDER) == "float32"
