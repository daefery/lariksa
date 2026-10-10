//! Where Lariksa keeps its files.
//!
//! Portable means everything lives next to the executable: recordings, the
//! database, downloaded models, and the native runtimes. Copy the folder to
//! another laptop and your meetings come with it. ``LARIKSA_HOME`` overrides the
//! root (development, tests).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The folder next to the running executable, or ``LARIKSA_HOME``.
    pub fn portable() -> Self {
        if let Some(home) = std::env::var_os("LARIKSA_HOME") {
            return Self::new(PathBuf::from(home));
        }
        let exe = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
        let root = exe.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        Self::new(simplify(root))
    }

    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn database(&self) -> PathBuf {
        self.data().join("lariksa.db")
    }
    pub fn recordings(&self) -> PathBuf {
        self.data().join("recordings")
    }
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }
    /// Downloaded native runtimes: llama.cpp and whisper.cpp.
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn meeting_dir(&self, id: &str) -> PathBuf {
        self.recordings().join(id)
    }

    pub fn ensure(self) -> std::io::Result<Self> {
        for dir in [self.data(), self.recordings(), self.models(), self.runtime()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(self)
    }
}

/// ``canonicalize`` on Windows returns ``\\?\C:\...``; child processes and the
/// WebView prefer the plain form.
fn simplify(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}
