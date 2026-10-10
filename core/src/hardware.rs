//! Which GPU the local AI runs on.
//!
//! llama.cpp and whisper.cpp share ggml's Vulkan backend, which lists every GPU.
//! On a laptop with an Intel iGPU plus an NVIDIA dGPU it defaults to the iGPU:
//! measured on an RTX 3050 Ti laptop, the summary model ran at 7 tokens/s on the
//! iGPU vs 62 on the dGPU, and Whisper was 6x slower. So the device is chosen
//! explicitly, once, from ``llama-server --list-devices``.

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static DEVICE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\w+\d+):\s*(.+?)\s*\((\d+) MiB,\s*(\d+) MiB free\)\s*$").unwrap());
static INTEGRATED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)intel.*(uhd|iris|hd graphics)|radeon\(tm\) graphics|radeon graphics|vega \d+ graphics").unwrap()
});
static SOFTWARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)llvmpipe|swiftshader|software|microsoft basic").unwrap());

/// Summary model weights (~2.4 GB) + 8k context KV cache + compute buffers.
pub const LLM_NEED_MIB: u64 = 3000;
/// Whisper large-v3-turbo q5_0 (~0.6 GB) + compute buffers.
pub const WHISPER_NEED_MIB: u64 = 1200;

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub name: String,
    pub description: String,
    pub total_mib: u64,
    pub free_mib: u64,
}

impl Device {
    pub fn integrated(&self) -> bool {
        INTEGRATED.is_match(&self.description)
    }

    /// whisper.cpp's ``--device N`` counts GPUs in the same order: "Vulkan1" -> 1.
    pub fn index(&self) -> Option<u32> {
        let digits: String = self.name.chars().rev().take_while(char::is_ascii_digit).collect();
        digits.chars().rev().collect::<String>().parse().ok()
    }
}

/// Parse ``llama-server --list-devices`` output.
pub fn parse_devices(listing: &str) -> Vec<Device> {
    listing
        .lines()
        .filter_map(|line| DEVICE_LINE.captures(line))
        .filter(|m| !SOFTWARE.is_match(&m[2]))
        .map(|m| Device { name: m[1].into(), description: m[2].into(), total_mib: m[3].parse().unwrap_or(0), free_mib: m[4].parse().unwrap_or(0) })
        .collect()
}

/// Best device, or None for CPU.
///
/// A discrete GPU wins even when partly full: llama.cpp's ``--fit`` keeps what
/// does not fit in system RAM, and a half-offloaded dGPU still beats a full iGPU.
/// An iGPU is used only when it is the only GPU and has room for the model;
/// it shares system RAM, so its "free" figure is real headroom.
pub fn choose_device(devices: &[Device], need_mib: u64) -> Option<&Device> {
    let discrete = devices.iter().filter(|d| !d.integrated()).max_by_key(|d| d.free_mib);
    if let Some(d) = discrete.filter(|d| d.free_mib >= need_mib / 2) {
        return Some(d);
    }
    devices.iter().find(|d| d.integrated() && d.free_mib >= need_mib)
}

pub fn list_devices(server: &Path) -> Vec<Device> {
    if !server.exists() {
        return vec![];
    }
    match crate::process::command(server).arg("--list-devices").output() {
        Ok(out) => {
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            parse_devices(&text)
        }
        Err(e) => {
            log::warn!("Could not list GPU devices: {e}");
            vec![]
        }
    }
}

/// What the status bar says: "GPU (NVIDIA GeForce RTX 3050 Ti Laptop GPU)" or "CPU".
pub fn label(device: Option<&Device>) -> String {
    device.map_or_else(|| "CPU".to_string(), |d| format!("GPU ({})", d.description))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "load_backend: loaded Vulkan backend from ggml-vulkan.dll
Available devices:
  Vulkan0: Intel(R) UHD Graphics (8054 MiB, 7395 MiB free)
  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3962 MiB, 3367 MiB free)
";

    #[test]
    fn parse() {
        let d = parse_devices(LISTING);
        assert_eq!(d.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["Vulkan0", "Vulkan1"]);
        assert!(d[1].description.starts_with("NVIDIA"));
        assert_eq!(d[1].free_mib, 3367);
        assert_eq!(d[1].index(), Some(1));
    }

    #[test]
    fn discrete_gpu_preferred_over_integrated_even_with_less_memory() {
        let d = parse_devices(LISTING);
        assert_eq!(choose_device(&d, 3000).unwrap().name, "Vulkan1");
        assert_eq!(choose_device(&d, WHISPER_NEED_MIB).unwrap().name, "Vulkan1");
    }

    #[test]
    fn integrated_gpu_used_when_it_is_the_only_one() {
        let only = parse_devices("  Vulkan0: Intel(R) Iris(R) Xe Graphics (8000 MiB, 6000 MiB free)\n");
        assert_eq!(choose_device(&only, 3000).unwrap().name, "Vulkan0");
    }

    #[test]
    fn cpu_when_no_gpu_has_room() {
        let small = parse_devices("  Vulkan0: AMD Radeon(TM) Graphics (2048 MiB, 1500 MiB free)\n");
        assert!(choose_device(&small, 3000).is_none());
        assert!(choose_device(&[], 3000).is_none());
    }

    #[test]
    fn software_renderers_ignored() {
        let l = parse_devices("  Vulkan0: llvmpipe (LLVM 15.0.7, 256 bits) (32000 MiB, 30000 MiB free)\n");
        assert!(choose_device(&l, 3000).is_none());
    }

    #[test]
    fn labels() {
        let d = parse_devices(LISTING);
        assert_eq!(label(choose_device(&d, 3000)), "GPU (NVIDIA GeForce RTX 3050 Ti Laptop GPU)");
        assert_eq!(label(None), "CPU");
    }
}
