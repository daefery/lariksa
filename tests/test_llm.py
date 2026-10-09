from lariksa.llm import choose_device, parse_devices

LISTING = """\
load_backend: loaded Vulkan backend from ggml-vulkan.dll
Available devices:
  Vulkan0: Intel(R) UHD Graphics (8054 MiB, 7395 MiB free)
  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3962 MiB, 3367 MiB free)
"""


def test_parse_devices():
    devices = parse_devices(LISTING)
    assert [d.name for d in devices] == ["Vulkan0", "Vulkan1"]
    assert devices[1].description.startswith("NVIDIA")
    assert devices[1].free_mib == 3367


def test_discrete_gpu_preferred_over_integrated_even_with_less_memory():
    assert choose_device(parse_devices(LISTING), need_mib=3000) == "Vulkan1"


def test_integrated_gpu_used_when_it_is_the_only_one():
    only_igpu = parse_devices("  Vulkan0: Intel(R) Iris(R) Xe Graphics (8000 MiB, 6000 MiB free)\n")
    assert choose_device(only_igpu, need_mib=3000) == "Vulkan0"


def test_cpu_when_no_gpu_has_room():
    small = parse_devices("  Vulkan0: AMD Radeon(TM) Graphics (2048 MiB, 1500 MiB free)\n")
    assert choose_device(small, need_mib=3000) == ""
    assert choose_device([], need_mib=3000) == ""


def test_software_renderers_ignored():
    llvmpipe = parse_devices("  Vulkan0: llvmpipe (LLVM 15.0.7, 256 bits) (32000 MiB, 30000 MiB free)\n")
    assert choose_device(llvmpipe, need_mib=3000) == ""
