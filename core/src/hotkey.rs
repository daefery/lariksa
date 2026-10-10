//! A system-wide keyboard shortcut (Windows RegisterHotKey).
//!
//! The hotkey works while Zoom or Meet has focus: RegisterHotKey delivers
//! WM_HOTKEY to a thread message queue owned by this process, so a small
//! background thread with its own message loop is enough. If the combination is
//! already taken by another app, registration fails and the caller is told,
//! rather than silently doing nothing.

use std::sync::Arc;

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

/// Default: Ctrl+Shift+R ("Rekam"). Unused by Zoom, Meet in Chrome/Edge and Teams.
pub const DEFAULT_HOTKEY: &str = "Ctrl+Shift+R";

fn key_code(key: &str) -> Option<u32> {
    let k = key.to_uppercase();
    let c = k.chars().next()?;
    if k.len() == 1 && (c.is_ascii_uppercase() || c.is_ascii_digit()) {
        return Some(c as u32);
    }
    if k == "SPACE" {
        return Some(0x20);
    }
    let n: u32 = k.strip_prefix('F')?.parse().ok()?;
    (1..=12).contains(&n).then_some(0x6F + n)
}

/// "Ctrl+Shift+R" -> (modifiers, virtual-key). Errors are shown to the user as-is.
pub fn parse(combo: &str) -> Result<(u32, u32), String> {
    let compact = combo.replace(' ', "");
    let parts: Vec<&str> = compact.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return Err("Pintasan butuh minimal satu tombol pengubah dan satu tombol, misal Ctrl+Shift+R.".into());
    }
    let mut mods = 0;
    for p in &parts[..parts.len() - 1] {
        mods |= match p.to_lowercase().as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "alt" => MOD_ALT,
            "shift" => MOD_SHIFT,
            "win" => MOD_WIN,
            _ => return Err(format!("Tombol pengubah tidak dikenal: {p}")),
        };
    }
    let last = parts[parts.len() - 1];
    let key = key_code(last).ok_or_else(|| format!("Tombol tidak dikenal: {last}"))?;
    if mods & (MOD_CONTROL | MOD_ALT | MOD_WIN) == 0 {
        return Err("Pakai Ctrl, Alt, atau Win agar tidak bentrok dengan mengetik.".into());
    }
    Ok((mods, key))
}

pub type Callback = Arc<dyn Fn() + Send + Sync>;

/// A registered shortcut; dropping it unregisters.
pub struct GlobalHotkey {
    pub combo: String,
    #[cfg(windows)]
    thread_id: u32,
    #[cfg(windows)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl GlobalHotkey {
    /// Register and listen. The error is a message for the UI.
    pub fn start(combo: &str, callback: Callback) -> Result<GlobalHotkey, String> {
        let (mods, vk) = parse(combo)?;
        imp::start(combo, mods, vk, callback)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn start(_: &str, _: u32, _: u32, _: Callback) -> Result<GlobalHotkey, String> {
        Err("Pintasan global hanya tersedia di Windows.".into())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, PostThreadMessageW, MSG, WM_HOTKEY, WM_QUIT};
    use windows::Win32::Foundation::{LPARAM, WPARAM};

    pub fn start(combo: &str, mods: u32, vk: u32, callback: Callback) -> Result<GlobalHotkey, String> {
        let (tx, rx) = mpsc::channel::<Result<u32, String>>();
        let label = combo.to_string();
        let busy = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("lariksa-hotkey".into())
            .spawn(move || unsafe {
                let tid = GetCurrentThreadId();
                if RegisterHotKey(None, 1, HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT, vk).is_err() {
                    let _ = tx.send(Err(format!("{label} sudah dipakai aplikasi lain. Pilih pintasan lain di Pengaturan.")));
                    return;
                }
                log::info!("Global hotkey {label} registered");
                let _ = tx.send(Ok(tid));
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                    if msg.message == WM_HOTKEY {
                        // Run the action on its own thread: this thread must keep pumping
                        // messages. Presses during a running action are ignored.
                        if busy.swap(true, Ordering::SeqCst) {
                            log::info!("Hotkey pressed while the previous action is still running; ignored");
                            continue;
                        }
                        let (cb, busy) = (callback.clone(), busy.clone());
                        std::thread::spawn(move || {
                            cb();
                            busy.store(false, Ordering::SeqCst);
                        });
                    }
                }
                let _ = UnregisterHotKey(None, 1);
            })
            .map_err(|e| e.to_string())?;
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(Ok(thread_id)) => Ok(GlobalHotkey { combo: combo.to_string(), thread_id, thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Pintasan tidak bisa didaftarkan.".into()),
        }
    }

    impl Drop for GlobalHotkey {
        fn drop(&mut self) {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_parse() {
        assert_eq!(parse("Ctrl+Shift+R"), Ok((0x2 | 0x4, 'R' as u32)));
        assert_eq!(parse("alt + F10"), Ok((0x1, 0x79)));
        for bad in ["R", "Shift+R", "Ctrl+Foo", "Hyper+R"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
