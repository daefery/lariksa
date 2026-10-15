//! Downloadable assets: models and native runtimes, pinned and verified.
//!
//! The app ships small; the heavy parts are fetched on first run into the
//! portable folder. Every URL is pinned to an exact revision and every large file
//! to a SHA-256, so a moved or tampered upstream file fails loudly instead of
//! loading silently.

use anyhow::{anyhow, bail, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::paths::Paths;
use crate::i18n::tr;

const HF: &str = "https://huggingface.co";
const WHISPER_CPP: (&str, &str) = ("ggerganov/whisper.cpp", "5359861c739e955e79d9a303bcbc70fb988958b1");
const VAD: (&str, &str) = ("ggml-org/whisper-vad", "9ffd54a1e1ee413ddf265af9913beaf518d1639b");
const QWEN: (&str, &str) = ("unsloth/Qwen3-4B-Instruct-2507-GGUF", "a06e946bb6b655725eafa393f4a9745d460374c9");
const LLAMA_TAG: &str = "b11514";
const WHISPER_TAG: &str = "b5454";

#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub url: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unpack {
    Files,
    Zip,
    Tar,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Base {
    Models,
    Runtime,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub key: &'static str,
    /// (Indonesian, English)
    pub label: (&'static str, &'static str),
    pub base: Base,
    pub folder: &'static str,
    pub files: Vec<RemoteFile>,
    pub unpack: Unpack,
    /// For archives: keep only members whose path contains one of these parts.
    pub keep: Vec<&'static str>,
    /// A file that proves the asset is installed, relative to its folder.
    pub marker: String,
}

impl Asset {
    /// The name shown on screen, in the display language.
    pub fn label(&self) -> &'static str {
        tr(self.label.0, self.label.1)
    }

    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn path(&self, paths: &Paths) -> PathBuf {
        match self.base {
            Base::Models => paths.models(),
            Base::Runtime => paths.runtime(),
        }
        .join(self.folder)
    }

    pub fn installed(&self, paths: &Paths) -> bool {
        self.path(paths).join(&self.marker).exists()
    }
}

fn hf((repo, rev): (&str, &str), name: &str, size: u64, sha: &str) -> RemoteFile {
    RemoteFile { url: format!("{HF}/{repo}/resolve/{rev}/{name}"), name: name.into(), size, sha256: sha.into() }
}

fn gh(repo: &str, tag: &str, asset: &str, local: &str, size: u64, sha: &str) -> RemoteFile {
    RemoteFile {
        url: format!("https://github.com/ggml-org/{repo}/releases/download/{tag}/{asset}"),
        name: local.into(),
        size,
        sha256: sha.into(),
    }
}

pub const WHISPER_TURBO: &str = "whisper-turbo";
pub const WHISPER_SMALL: &str = "whisper-small";
pub const VAD_MODEL: &str = "vad";
pub const SUMMARY_MODEL: &str = "llm";
pub const LLAMA_RUNTIME: &str = "llama";
pub const WHISPER_RUNTIME: &str = "whisper";

pub const TURBO_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
pub const SMALL_FILE: &str = "ggml-small-q8_0.bin";
pub const VAD_FILE: &str = "ggml-silero-v6.2.0.bin";
pub const QWEN_FILE: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

pub fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Every downloadable asset, in the order the UI lists them.
pub fn assets() -> Vec<Asset> {
    let windows = cfg!(windows);
    vec![
        Asset {
            key: WHISPER_TURBO,
            label: ("Model transkripsi (Whisper large-v3-turbo)", "Transcription model (Whisper large-v3-turbo)"),
            base: Base::Models,
            folder: "whisper-cpp",
            files: vec![hf(WHISPER_CPP, TURBO_FILE, 574_041_195, "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2")],
            unpack: Unpack::Files,
            keep: vec![],
            marker: TURBO_FILE.into(),
        },
        Asset {
            key: WHISPER_SMALL,
            label: ("Model transkripsi ringan (Whisper small)", "Light transcription model (Whisper small)"),
            base: Base::Models,
            folder: "whisper-cpp",
            files: vec![hf(WHISPER_CPP, SMALL_FILE, 264_464_607, "49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f")],
            unpack: Unpack::Files,
            keep: vec![],
            marker: SMALL_FILE.into(),
        },
        // Silence detection: skips quiet stretches, which is faster and stops Whisper
        // inventing text over the empty system track.
        Asset {
            key: VAD_MODEL,
            label: ("Deteksi suara (Silero VAD)", "Speech detection (Silero VAD)"),
            base: Base::Models,
            folder: "whisper-cpp",
            files: vec![hf(VAD, VAD_FILE, 885_098, "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987")],
            unpack: Unpack::Files,
            keep: vec![],
            marker: VAD_FILE.into(),
        },
        Asset {
            key: SUMMARY_MODEL,
            label: ("Model notulen (Qwen3 4B Instruct)", "Minutes model (Qwen3 4B Instruct)"),
            base: Base::Models,
            folder: "llm",
            files: vec![hf(QWEN, QWEN_FILE, 2_497_281_120, "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597")],
            unpack: Unpack::Files,
            keep: vec![],
            marker: QWEN_FILE.into(),
        },
        // Vulkan runs on NVIDIA, AMD and Intel graphics, and the build carries the CPU
        // backend too, so one download covers every Windows laptop. Whisper uses the
        // same ggml version, so its runtime borrows this build's Vulkan backend.
        Asset {
            key: LLAMA_RUNTIME,
            label: ("Mesin AI lokal (llama.cpp)", "Local AI engine (llama.cpp)"),
            base: Base::Runtime,
            folder: "llama",
            files: vec![if windows {
                gh("llama.cpp", LLAMA_TAG, &format!("llama-{LLAMA_TAG}-bin-win-vulkan-x64.zip"), "llama-win-vulkan-x64.zip",
                   33_446_009, "e1eaddcde761b1f839b9c6d5924b97f2698fba22068ca3b63f129c3caf55b88f")
            } else {
                gh("llama.cpp", LLAMA_TAG, &format!("llama-{LLAMA_TAG}-bin-ubuntu-x64.tar.gz"), "llama-ubuntu-x64.tar.gz",
                   17_793_295, "b2b617fb28e9d1444235c30bb09a3704789a1840e16ead58a5f7be6eeae8beef")
            }],
            unpack: if windows { Unpack::Zip } else { Unpack::Tar },
            keep: vec![],
            marker: exe("llama-server"),
        },
        Asset {
            key: WHISPER_RUNTIME,
            label: ("Mesin transkripsi (whisper.cpp)", "Transcription engine (whisper.cpp)"),
            base: Base::Runtime,
            folder: "whisper",
            files: vec![if windows {
                gh("whisper.cpp", WHISPER_TAG, "whisper-bin-x64.zip", "whisper-bin-x64.zip",
                   8_928_640, "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb")
            } else {
                gh("whisper.cpp", WHISPER_TAG, "whisper-bin-ubuntu-x64.tar.gz", "whisper-bin-ubuntu-x64.tar.gz",
                   10_364_195, "a72becf15d7917f990f6313867a52638b82b7f9ef237fb0c980dac56a135781c")
            }],
            unpack: if windows { Unpack::Zip } else { Unpack::Tar },
            // Only the CLI and the libraries; the archive also carries demos and tests.
            keep: if windows {
                vec!["whisper-cli.exe", "whisper.dll", "ggml", "LICENSE"]
            } else {
                vec!["whisper-cli", "libwhisper", "libggml", "LICENSE"]
            },
            marker: exe("whisper-cli"),
        },
    ]
}

pub fn asset(key: &str) -> Option<Asset> {
    assets().into_iter().find(|a| a.key == key)
}

pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn agent() -> ureq::Agent {
    http_agent(Duration::from_secs(120))
}

/// An HTTPS-capable client; Windows uses the system certificate store.
pub fn http_agent(read_timeout: Duration) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(30)).timeout_read(read_timeout);
    #[cfg(windows)]
    let builder = builder.tls_connector(std::sync::Arc::new(native_tls::TlsConnector::new().expect("TLS")));
    builder.build()
}

/// Download one file with resume support, then verify its hash.
fn fetch(remote: &RemoteFile, target: &Path, done_before: u64, total: u64, progress: Progress, cancelled: &dyn Fn() -> bool) -> Result<()> {
    let partial = target.with_file_name(format!("{}.part", remote.name));
    let mut offset = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
    let mut req = agent().get(&remote.url);
    if offset > 0 {
        req = req.set("Range", &format!("bytes={offset}-"));
    }
    match req.call() {
        Err(ureq::Error::Status(416, _)) => {} // already complete
        Err(ureq::Error::Status(code, _)) => bail!("{}: HTTP {code}", remote.name),
        Err(e) => bail!("{}: {e}", remote.name),
        Ok(response) => {
            if response.status() == 200 {
                offset = 0; // server ignored the range; start over
            }
            let mut out = if offset > 0 {
                fs::OpenOptions::new().append(true).open(&partial)?
            } else {
                File::create(&partial)?
            };
            let mut reader = response.into_reader();
            let mut buf = vec![0u8; 1 << 20];
            let mut written = offset;
            loop {
                if cancelled() {
                    bail!("{}", tr("Unduhan dibatalkan.", "Download cancelled."));
                }
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                written += n as u64;
                progress(done_before + written, total);
            }
        }
    }
    if !remote.sha256.is_empty() && sha256_file(&partial)? != remote.sha256 {
        let _ = fs::remove_file(&partial);
        bail!("{}: {}", remote.name, tr("file rusak (hash tidak cocok). Coba unduh lagi.", "file is damaged (hash mismatch). Try downloading again."));
    }
    fs::rename(&partial, target)?;
    Ok(())
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Extract an archive flat into ``dest`` (the runtime's own folder layout is not needed).
fn unpack(archive: &Path, dest: &Path, kind: Unpack, keep: &[&str]) -> Result<()> {
    let wanted = |name: &str| keep.is_empty() || keep.iter().any(|k| name.contains(k));
    match kind {
        Unpack::Files => {}
        Unpack::Zip => {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
            for i in 0..zip.len() {
                let mut member = zip.by_index(i)?;
                if member.is_dir() || !wanted(member.name()) {
                    continue;
                }
                let name = file_name(member.name()).to_string();
                let mut out = File::create(dest.join(name))?;
                std::io::copy(&mut member, &mut out)?;
            }
        }
        Unpack::Tar => {
            // Linux runtimes (development and tests only): the system tar keeps symlinks.
            let tmp = dest.join(".unpack");
            fs::create_dir_all(&tmp)?;
            let ok = std::process::Command::new("tar").arg("-xzf").arg(archive).arg("-C").arg(&tmp).status()?.success();
            if !ok {
                bail!("tar gagal membuka {}", archive.display());
            }
            for entry in walk(&tmp)? {
                let name = entry.file_name().unwrap().to_string_lossy().to_string();
                if wanted(&entry.to_string_lossy()) {
                    let target = dest.join(&name);
                    let _ = fs::remove_file(&target);
                    fs::rename(&entry, &target)?;
                }
            }
            fs::remove_dir_all(&tmp)?;
        }
    }
    Ok(())
}

fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() && !path.is_symlink() {
            out.extend(walk(&path)?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

/// The Vulkan backend for whisper.cpp comes from the llama.cpp build (same ggml
/// version); the whisper.cpp Windows release ships only CPU and CUDA builds.
pub fn link_vulkan(paths: &Paths) -> Result<()> {
    let (Some(llama), Some(whisper)) = (asset(LLAMA_RUNTIME), asset(WHISPER_RUNTIME)) else { return Ok(()) };
    let name = if cfg!(windows) { "ggml-vulkan.dll" } else { "libggml-vulkan.so" };
    let src = llama.path(paths).join(name);
    let dst = whisper.path(paths).join(name);
    if whisper.installed(paths) && src.exists() && !dst.exists() {
        fs::copy(&src, &dst)?;
    }
    Ok(())
}

/// Download (resuming if interrupted), verify and unpack an asset.
pub fn install(asset: &Asset, paths: &Paths, progress: Progress, cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
    let dest = asset.path(paths);
    if !asset.installed(paths) {
        fs::create_dir_all(&dest)?;
        let staging = paths.root.join("downloads");
        fs::create_dir_all(&staging)?;
        let total = asset.size();
        let mut done = 0;
        for remote in &asset.files {
            let target = if asset.unpack == Unpack::Files { dest.join(&remote.name) } else { staging.join(&remote.name) };
            if !target.exists() {
                fetch(remote, &target, done, total, progress, cancelled)?;
            }
            done += remote.size;
            if asset.unpack != Unpack::Files {
                unpack(&target, &dest, asset.unpack, &asset.keep)?;
                let _ = fs::remove_file(&target);
            }
        }
        if !asset.installed(paths) {
            return Err(anyhow!("{}: {} {}", asset.label(), tr("unduhan selesai tapi file ini tidak ditemukan:", "download finished but this file is missing:"), asset.marker));
        }
    }
    link_vulkan(paths)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_pinned() {
        let all = assets();
        let mut keys: Vec<_> = all.iter().map(|a| a.key).collect();
        keys.dedup();
        assert_eq!(keys.len(), all.len());
        for a in &all {
            for f in &a.files {
                assert_eq!(f.sha256.len(), 64, "{}", f.name);
                assert!(!f.url.contains("/main/"), "{} must pin a revision", f.url);
            }
        }
    }

    #[test]
    fn whisper_models_share_one_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let turbo = asset(WHISPER_TURBO).unwrap();
        assert_eq!(turbo.path(&paths), dir.path().join("models").join("whisper-cpp"));
        assert!(!turbo.installed(&paths));
        fs::create_dir_all(turbo.path(&paths)).unwrap();
        fs::write(turbo.path(&paths).join(TURBO_FILE), b"x").unwrap();
        assert!(turbo.installed(&paths));
    }

    #[test]
    fn zip_unpack_keeps_only_wanted_files_flat() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for name in ["Release/whisper-cli.exe", "Release/ggml-base.dll", "Release/SDL2.dll", "Release/stream.exe"] {
                z.start_file(name, opts).unwrap();
                z.write_all(b"x").unwrap();
            }
            z.finish().unwrap();
        }
        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        unpack(&archive, &dest, Unpack::Zip, &["whisper-cli.exe", "ggml"]).unwrap();
        let mut names: Vec<_> = fs::read_dir(&dest).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["ggml-base.dll", "whisper-cli.exe"]);
    }
}
