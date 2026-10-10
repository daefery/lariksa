//! Lariksa desktop: the main window, the recording pill, the shortcut and the
//! taskbar progress, on top of ``lariksa-core``.
//!
//! Each command is a one-liner on purpose: the UI contract is the service.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use lariksa_core::hotkey::GlobalHotkey;
use lariksa_core::overlay::{self, Inputs, Overlay, Taskbar, Toggle, PILL_SIZE};
use lariksa_core::paths::Paths;
use lariksa_core::service::{import_types, LariksaService};
use lariksa_core::store::parse_iso;

mod logging;

const MAIN: &str = "main";
const PILL: &str = "pill";

struct App {
    svc: Arc<LariksaService>,
    hotkey: Mutex<Option<GlobalHotkey>>,
    hotkey_error: Mutex<String>,
    overlay: Mutex<Overlay>,
}

type S<'a> = State<'a, Arc<App>>;

// -- the JavaScript API (same names and shapes as the Python version) ---------

#[tauri::command]
fn status(app: S) -> Value {
    app.svc.status()
}

#[tauri::command]
fn meetings(app: S, query: Option<String>) -> Value {
    app.svc.meetings(&query.unwrap_or_default())
}

#[tauri::command]
fn meeting(app: S, meeting_id: String) -> Value {
    app.svc.meeting(&meeting_id)
}

#[tauri::command]
async fn start_recording(app: S<'_>, title: Option<String>) -> Result<Value, ()> {
    Ok(app.svc.start_recording(&title.unwrap_or_default()))
}

#[tauri::command]
async fn stop_recording(app: S<'_>) -> Result<Value, ()> {
    Ok(app.svc.stop_recording())
}

#[tauri::command]
fn rename(app: S, meeting_id: String, title: String) -> Value {
    app.svc.rename(&meeting_id, &title)
}

#[tauri::command]
fn save_notulen(app: S, meeting_id: String, data: Value) -> Value {
    app.svc.save_notulen(&meeting_id, &data)
}

#[tauri::command]
fn retry(app: S, meeting_id: String, summary_only: Option<bool>) -> Value {
    app.svc.retry(&meeting_id, summary_only.unwrap_or(false))
}

#[tauri::command]
fn delete(app: S, meeting_id: String) -> Value {
    app.svc.delete(&meeting_id)
}

#[tauri::command]
fn save_settings(app: S, values: Map<String, Value>) -> Value {
    let before = app.svc.store.settings().get("hotkey").cloned();
    let saved = match app.svc.save_settings(&values) {
        Ok(s) => s,
        Err(e) => return json!({"ok": false, "error": e.to_string()}),
    };
    if saved.get("hotkey") != before.as_ref() {
        restart_hotkey(&app, saved.get("hotkey").and_then(Value::as_str).unwrap_or(""));
    }
    let mut out = saved;
    out.insert("hotkey_error".into(), json!(*app.hotkey_error.lock().unwrap()));
    Value::Object(out)
}

#[tauri::command]
fn download(app: S, keys: Vec<String>) -> Value {
    app.svc.download(keys)
}

#[tauri::command]
async fn import_audio(app: S<'_>, window: WebviewWindow) -> Result<Value, ()> {
    let exts: Vec<&str> = import_types().iter().map(|e| e.trim_start_matches('.')).collect();
    let picked = rfd::FileDialog::new().set_parent(&window).add_filter("Rekaman atau transkrip", &exts).pick_file();
    Ok(match picked {
        Some(path) => app.svc.import_file(&path, ""),
        None => json!({"ok": false, "error": ""}),
    })
}

fn safe_filename(title: &str) -> String {
    let cleaned: String = title.chars().filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && *c as u32 >= 0x20).collect();
    let cleaned = cleaned.trim().trim_end_matches('.').to_string();
    let name = if cleaned.is_empty() { "Notulen".to_string() } else { cleaned };
    name.chars().take(80).collect()
}

#[tauri::command]
async fn export(app: S<'_>, window: WebviewWindow, meeting_id: String, kind: String) -> Result<Value, ()> {
    let Some(m) = app.svc.store.get(&meeting_id) else {
        return Ok(json!({"ok": false, "error": "Rapat tidak ditemukan."}));
    };
    let (ext, filter) = if kind == "docx" { ("docx", "Word") } else { ("md", "Markdown") };
    let date = parse_iso(&m.created_at).map(|t| t.format("%Y-%m-%d").to_string()).unwrap_or_default();
    let name = format!("{} - {date}.{ext}", safe_filename(&m.title));
    let documents = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents"));
    let mut dialog = rfd::FileDialog::new().set_parent(&window).set_file_name(&name).add_filter(filter, &[ext]);
    if let Some(dir) = documents.filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(target) = dialog.save_file() else {
        return Ok(json!({"ok": false, "error": ""}));
    };
    Ok(if ext == "docx" { app.svc.export_docx(&meeting_id, &target) } else { app.svc.export_markdown(&meeting_id, &target) })
}

#[tauri::command]
fn copy_text(text: String) -> Value {
    match clipboard::set(&text) {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

#[tauri::command]
fn hotkey_label(app: S) -> String {
    app.hotkey.lock().unwrap().as_ref().map(|h| h.combo.clone()).unwrap_or_default()
}

#[tauri::command]
fn hotkey_error(app: S) -> String {
    app.hotkey_error.lock().unwrap().clone()
}

#[tauri::command]
fn show_main(handle: tauri::AppHandle) {
    show_main_window(&handle);
}

#[tauri::command]
fn hide_pill(app: S, handle: tauri::AppHandle) {
    app.overlay.lock().unwrap().hide_pill();
    if let Some(p) = handle.get_webview_window(PILL) {
        let _ = p.hide();
    }
}

#[tauri::command]
fn start_drag(window: WebviewWindow) {
    let _ = window.start_dragging();
}

// -- windows, shortcut, indicator ---------------------------------------------

fn show_main_window(handle: &tauri::AppHandle) {
    if let Some(w) = handle.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn restart_hotkey(app: &Arc<App>, combo: &str) {
    let handle = HANDLE.get().cloned();
    *app.hotkey.lock().unwrap() = None; // unregister the old one first
    let error = if combo.is_empty() {
        String::new()
    } else {
        let svc = app.svc.clone();
        let callback = Arc::new(move || {
            if overlay::toggle_recording(&svc) == Toggle::ShowMain {
                if let Some(h) = &handle {
                    let h2 = h.clone();
                    let _ = h.run_on_main_thread(move || show_main_window(&h2));
                }
            }
        });
        match GlobalHotkey::start(combo, callback) {
            Ok(hk) => {
                *app.hotkey.lock().unwrap() = Some(hk);
                String::new()
            }
            Err(e) => {
                log::warn!("Hotkey {combo} unavailable: {e}");
                e
            }
        }
    };
    *app.hotkey_error.lock().unwrap() = error;
}

static HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

fn set_taskbar(window: &WebviewWindow, t: Taskbar) {
    use tauri::window::{ProgressBarState, ProgressBarStatus};
    let state = match t {
        Taskbar::None => ProgressBarState { status: Some(ProgressBarStatus::None), progress: None },
        Taskbar::Indeterminate => ProgressBarState { status: Some(ProgressBarStatus::Indeterminate), progress: None },
        Taskbar::Normal(p) => ProgressBarState { status: Some(ProgressBarStatus::Normal), progress: Some((p * 100.0).round() as u64) },
        Taskbar::Error => ProgressBarState { status: Some(ProgressBarStatus::Error), progress: Some(100) },
    };
    let _ = window.set_progress_bar(state);
}

/// Follow the service and keep the pill and the taskbar in sync.
fn watch(app: Arc<App>, handle: tauri::AppHandle) {
    let clock = Instant::now();
    let mut was_recording = false;
    let mut last: Option<(bool, Taskbar)> = None;
    while !app.svc.cancelled() {
        std::thread::sleep(Duration::from_millis(500));
        let inputs = Inputs::of(&app.svc);
        let decision = app.overlay.lock().unwrap().tick(&inputs, was_recording, clock.elapsed().as_secs_f64());
        was_recording = inputs.recording;
        if last == Some((decision.pill_visible, decision.taskbar)) {
            continue;
        }
        last = Some((decision.pill_visible, decision.taskbar));
        let h = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(main) = h.get_webview_window(MAIN) {
                set_taskbar(&main, decision.taskbar);
            }
            if let Some(pill) = h.get_webview_window(PILL) {
                // The pill is not focusable, so showing it never takes focus from Zoom/Meet.
                let _ = if decision.pill_visible { pill.show() } else { pill.hide() };
            }
        });
    }
}

fn build_windows(app: &tauri::App) -> tauri::Result<()> {
    let data = app.state::<Arc<App>>().svc.paths.data().join("webview");
    let main = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Lariksa")
        .inner_size(1180.0, 780.0)
        .min_inner_size(860.0, 600.0)
        .background_color(tauri::window::Color(0xF7, 0xF8, 0xF4, 0xFF))
        .data_directory(data.clone())
        .initialization_script(include_str!("bridge.js"))
        .disable_drag_drop_handler()
        .build()?;

    // The recording indicator: small, always on top, never takes focus from Zoom/Meet.
    let (x, y) = main
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let scale = m.scale_factor();
            overlay::pill_position(m.position().x as f64 / scale, m.position().y as f64 / scale, m.size().width as f64 / scale)
        })
        .unwrap_or((600.0, 16.0));
    WebviewWindowBuilder::new(app, PILL, WebviewUrl::App("pill.html".into()))
        .title("Lariksa · rekaman")
        .inner_size(PILL_SIZE.0, PILL_SIZE.1)
        .position(x, y)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .shadow(true)
        .background_color(tauri::window::Color(0x17, 0x24, 0x1E, 0xFF))
        .data_directory(data)
        .initialization_script(include_str!("bridge.js"))
        .disable_drag_drop_handler()
        .build()?;
    Ok(())
}

fn main() {
    let paths = match Paths::portable().ensure() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Lariksa cannot write next to the app: {e}");
            std::process::exit(1);
        }
    };
    logging::init(&paths.data().join("lariksa.log"));
    let svc = match LariksaService::new(paths) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Startup failed: {e:#}");
            std::process::exit(1);
        }
    };
    let app = Arc::new(App { svc, hotkey: Mutex::new(None), hotkey_error: Mutex::new(String::new()), overlay: Mutex::new(Overlay::default()) });

    tauri::Builder::default()
        .manage(app.clone())
        .invoke_handler(tauri::generate_handler![
            status, meetings, meeting, start_recording, stop_recording, rename, save_notulen, retry, delete,
            save_settings, download, import_audio, export, copy_text, hotkey_label, hotkey_error, show_main,
            hide_pill, start_drag
        ])
        .setup(move |tauri_app| {
            let _ = HANDLE.set(tauri_app.handle().clone());
            build_windows(tauri_app)?;
            let combo = app.svc.store.settings().get("hotkey").and_then(Value::as_str).unwrap_or("").to_string();
            restart_hotkey(&app, &combo);
            let (watcher, handle) = (app.clone(), tauri_app.handle().clone());
            std::thread::Builder::new().name("lariksa-overlay".into()).spawn(move || watch(watcher, handle))?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN {
                if let WindowEvent::CloseRequested { .. } = event {
                    // Never lose a meeting: closing mid-recording stops and saves it first.
                    let app = window.state::<Arc<App>>();
                    *app.hotkey.lock().unwrap() = None;
                    app.svc.shutdown();
                    window.app_handle().exit(0);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Lariksa");
}

mod clipboard {
    /// Plain-text clipboard, Unicode intact (WhatsApp messages carry bullets and accents).
    #[cfg(windows)]
    pub fn set(text: &str) -> Result<(), String> {
        use windows::Win32::Foundation::{HANDLE, HGLOBAL};
        use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
        const CF_UNICODETEXT: u32 = 13;
        let wide: Vec<u16> = text.replace("\r\n", "\n").replace('\n', "\r\n").encode_utf16().chain([0]).collect();
        unsafe {
            let mut opened = false;
            for _ in 0..10 {
                if OpenClipboard(None).is_ok() {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(30)); // another app holds it
            }
            if !opened {
                return Err("Clipboard sedang dipakai aplikasi lain.".into());
            }
            let result = (|| -> Result<(), String> {
                EmptyClipboard().map_err(|e| e.to_string())?;
                let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).map_err(|e| e.to_string())?;
                let ptr = GlobalLock(mem) as *mut u16;
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(mem);
                SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).map_err(|e| e.to_string())?;
                Ok(())
            })();
            let _ = CloseClipboard();
            result
        }
    }

    #[cfg(not(windows))]
    pub fn set(_: &str) -> Result<(), String> {
        Err("Clipboard hanya di Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn safe_filename() {
        assert_eq!(super::safe_filename("Rapat: Q4/budget? \"final\""), "Rapat Q4budget final");
        assert_eq!(super::safe_filename("..."), "Notulen");
        assert_eq!(super::safe_filename(&"x".repeat(300)).len(), 80);
    }
}
