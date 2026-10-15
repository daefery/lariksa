// Browser-only stand-in for the app's API (window.pywebview.api, see desktop/src/bridge.js), for UI previews and README
// screenshots without Windows. Loaded by scripts/preview.py, never shipped.
// Data comes from scripts/sample_meeting.json (fictional), injected as
// window.SAMPLE by preview.py.
//
// URL parameters:
//   s=home|recording|processing|setup   app state
//   open=card|processing|settings       view to open after load
//   tab=notulen|transcript              meeting tab
//   lang=id|en                          language of the sample meeting
//   theme=light|dark
//   llm=local|api                       who writes the minutes (Settings)
(() => {
  const params = new URLSearchParams(location.search);
  const scenario = params.get("s") || "home";
  const viaApi = params.get("llm") === "api";
  const lang = params.get("lang") === "en" ? "en" : "id";
  const sample = window.SAMPLE;
  const now = new Date();
  const iso = (minsAgo) => new Date(now - minsAgo * 60000).toISOString();

  const notulen = sample.minutes[lang];
  const transcript = sample.transcript[lang];
  const lines = transcript.map((u, i) => ({ n: i + 1, start: u.start, speaker: u.speaker, text: u.text }));
  const LABELS = {
    id: { summary: "Ringkasan", highlights: "Poin penting", actions: "Action items", decisions: "Keputusan",
      questions: "Pertanyaan terbuka", chapters: "Pembahasan", speakers: "Peserta", none: "(tidak ada)", unassigned: "Tanpa PIC" },
    en: { summary: "Summary", highlights: "Highlights", actions: "Action items", decisions: "Decisions",
      questions: "Open questions", chapters: "Discussion", speakers: "Speakers", none: "(none)", unassigned: "Unassigned" },
  };
  const meetings = sample.meetings.map((m) => ({
    title: notulen.title, preview: notulen.overview, ...m, created_at: iso(m.minutes_ago),
  }));

  // Mirrors core/src/catalog.rs.
  const assets = {
    "whisper-turbo": { label: "Model transkripsi (Whisper large-v3-turbo)", size: 574e6, installed: scenario !== "setup" },
    "whisper-small": { label: "Model transkripsi ringan (Whisper small)", size: 264e6, installed: false },
    vad: { label: "Deteksi suara (Silero VAD)", size: 885e3, installed: scenario !== "setup" },
    llm: { label: "Model notulen (Qwen3 4B Instruct)", size: 2497e6, installed: scenario !== "setup" },
    llama: { label: "Mesin AI lokal (llama.cpp)", size: 33e6, installed: scenario !== "setup" },
    whisper: { label: "Mesin transkripsi (whisper.cpp)", size: 9e6, installed: scenario !== "setup" },
  };
  // Mirrors core/src/service.rs: downloaded single-file models can be removed.
  for (const k of ["whisper-turbo", "whisper-small", "vad", "llm"]) assets[k].removable = true;
  const t0 = Date.now();
  const status = () => {
    const live = scenario === "recording";
    const el = (Date.now() - t0) / 1000 + 754;
    return {
      recording: live, recording_id: live ? "m3" : "", elapsed: live ? el : 0,
      levels: live ? { mic: 0.32, system: 0.55 } : {},
      silent: [],
      devices: live ? { mic: "Microphone Array (Realtek)", system: "Headphones (WH-1000XM4)" } : {},
      activity: { meeting_id: scenario === "processing" ? "m3" : "", stage: "summarizing", progress: 0.42,
        downloads: scenario === "setup" ? { "whisper-turbo": { label: "", done: 0.37 * 574e6, total: 574e6, error: "" } } : {} },
      queue: 0,
      compute: { label: "GPU (NVIDIA GeForce RTX 3050 Ti Laptop GPU)", device: "gpu", gpu: "NVIDIA GeForce RTX 3050 Ti Laptop GPU" },
      assets, needed: ["whisper-turbo", "vad", ...(viaApi ? [] : ["llm"]), "llama", "whisper"], ready: scenario !== "setup",
      settings: { language: "id", notulen_language: "auto", whisper_model: "auto",
        glossary: ["Budi", "Sari", "Dewi", "regression test", "staging"], theme: params.get("theme") || "light",
        ui_language: params.get("ui") === "en" ? "en" : "id", hotkey: "Ctrl+Shift+R", show_pill: true,
        llm_provider: viaApi ? "api" : "local", llm_api_url: viaApi ? "http://127.0.0.1:8317/v1" : "",
        llm_api_model: viaApi ? "gemini-3-flash" : "", llm_api_key_set: viaApi },
    };
  };
  // A full meeting payload (as returned by the real service) can be injected for previews.
  if (window.MEETING) meetings.unshift({ ...window.MEETING, created_at: iso(35) });
  const detail = (id) => {
    if (window.MEETING && id === window.MEETING.id) return { ...window.MEETING, created_at: iso(35) };
    const m = meetings.find((x) => x.id === id);
    const done = m.status === "done";
    return { ...m, language: lang, labels: LABELS[lang], error: "", transcript: done ? transcript : [],
      lines: done ? lines : [], notulen: done ? notulen : null, timings: done ? { transcribe_s: 84, summarize_s: 71 } : {},
      has_audio: done, audio_url: done ? "about:blank" : "", whatsapp: "", markdown: "" };
  };
  const ok = async () => ({ ok: true });
  window.pywebview = {
    api: {
      status: async () => status(),
      meetings: async (q) => meetings.filter((m) => !q || (m.title + m.preview).toLowerCase().includes(q.toLowerCase())),
      meeting: async (id) => detail(id),
      start_recording: ok, stop_recording: ok, rename: ok, save_notulen: ok, retry: ok, delete: ok,
      download: ok, export: ok, copy_text: ok, remove_asset: ok,
      check_llm_api: async () => ({ ok: true, models: ["claude-sonnet-4-6", "gemini-3-flash", "gpt-6-luna"] }),
      save_settings: async (v) => v,
      import_audio: async () => ({ ok: false, error: "" }),
      hotkey_label: async () => "Ctrl+Shift+R", hotkey_error: async () => "", show_main: ok, hide_pill: ok,
    },
  };
  window.addEventListener("load", () => {
    window.dispatchEvent(new Event("pywebviewready"));
    const open = params.get("open");
    const card = { card: 0, processing: 2 }[open];
    if (open) setTimeout(() => card === undefined
      ? document.querySelector(`[data-view="${open}"]`)?.click()
      : [...document.querySelectorAll(".cards .card")][card]?.click(), 300);
    if (params.get("tab")) setTimeout(() => document.querySelector(`[data-tab="${params.get("tab")}"]`)?.click(), 700);
  });
})();
