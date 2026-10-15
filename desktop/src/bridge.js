// The UI's API (window.pywebview.api.<name>(...)) on top of Tauri's invoke.
// The UI was written against this shape; keeping it means the screens did not
// have to change when the app moved from Python to Rust. Every method maps to
// one #[tauri::command] in desktop/src/main.rs with the same name and arguments.
(() => {
  const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args || {});
  const api = {
    status: () => invoke("status"),
    meetings: (query = "") => invoke("meetings", { query }),
    meeting: (meetingId) => invoke("meeting", { meetingId }),
    start_recording: (title = "") => invoke("start_recording", { title }),
    stop_recording: () => invoke("stop_recording"),
    rename: (meetingId, title) => invoke("rename", { meetingId, title }),
    save_notulen: (meetingId, data) => invoke("save_notulen", { meetingId, data }),
    retry: (meetingId, summaryOnly = false) => invoke("retry", { meetingId, summaryOnly }),
    delete: (meetingId) => invoke("delete", { meetingId }),
    save_settings: (values) => invoke("save_settings", { values }),
    download: (keys) => invoke("download", { keys }),
    remove_asset: (key) => invoke("remove_asset", { key }),
    check_llm_api: (url, key) => invoke("check_llm_api", { url, key }),
    import_audio: () => invoke("import_audio"),
    export: (meetingId, kind) => invoke("export", { meetingId, kind }),
    copy_text: (text) => invoke("copy_text", { text }),
    hotkey_label: () => invoke("hotkey_label"),
    hotkey_error: () => invoke("hotkey_error"),
    show_main: () => invoke("show_main"),
    hide_pill: () => invoke("hide_pill"),
  };
  window.pywebview = { api };

  // Frameless pill: elements marked as a drag region move the window.
  document.addEventListener("mousedown", (e) => {
    if (e.button === 0 && e.target.closest?.(".pywebview-drag-region")) {
      e.preventDefault();
      invoke("start_drag");
    }
  });

  const ready = () => window.dispatchEvent(new Event("pywebviewready"));
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", ready);
  else setTimeout(ready, 0);
})();
