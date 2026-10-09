// Browser-only stand-in for window.pywebview.api, for UI previews and README
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
(() => {
  const params = new URLSearchParams(location.search);
  const scenario = params.get("s") || "home";
  const lang = params.get("lang") === "en" ? "en" : "id";
  const sample = window.SAMPLE;
  const now = new Date();
  const iso = (minsAgo) => new Date(now - minsAgo * 60000).toISOString();

  const notulen = { ...sample.notulen[lang], language: lang };
  const transcript = sample.transcript[lang];
  const LABELS = {
    id: { summary: "Ringkasan", topics: "Pembahasan", decisions: "Keputusan", action_items: "Action items",
      open_questions: "Pertanyaan terbuka", none: "(tidak ada)", task: "Tugas", owner: "PIC", due: "Deadline" },
    en: { summary: "Summary", topics: "Discussion", decisions: "Decisions", action_items: "Action items",
      open_questions: "Open questions", none: "(none)", task: "Task", owner: "Owner", due: "Due" },
  };
  const meetings = sample.meetings.map((m) => ({
    title: notulen.title, preview: notulen.summary, ...m, created_at: iso(m.minutes_ago),
  }));

  const assets = {
    "whisper-turbo": { label: "Model transkripsi (Whisper large-v3-turbo)", size: 1622e6, installed: scenario !== "setup" },
    "whisper-small": { label: "Model transkripsi ringan (Whisper small)", size: 486e6, installed: false },
    llm: { label: "Model notulen (Qwen3 4B Instruct)", size: 2497e6, installed: scenario !== "setup" },
    llama: { label: "Mesin AI lokal (llama.cpp)", size: 33e6, installed: scenario !== "setup" },
    cuda: { label: "Akselerasi GPU NVIDIA (cuBLAS + cuDNN)", size: 1260e6, installed: scenario !== "setup" },
  };
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
        downloads: scenario === "setup" ? { "whisper-turbo": { label: "", done: 0.37 * 1622e6, total: 1622e6, error: "" } } : {} },
      queue: 0,
      compute: { label: "GPU (RTX 3050 Ti Laptop GPU)", device: "cuda", gpu: "RTX 3050 Ti", cuda_ready: true },
      assets, needed: ["whisper-turbo", "llm", "llama", "cuda"], ready: scenario !== "setup",
      settings: { language: "id", notulen_language: "auto", whisper_model: "auto",
        glossary: ["Budi", "Sari", "Dewi", "regression test", "staging"], theme: params.get("theme") || "light" },
    };
  };
  const detail = (id) => {
    const m = meetings.find((x) => x.id === id);
    const done = m.status === "done";
    return { ...m, language: lang, labels: LABELS[lang], error: "", transcript: done ? transcript : [],
      notulen: done ? notulen : null, timings: done ? { transcribe_s: 84, summarize_s: 71 } : {},
      whatsapp: "", markdown: "" };
  };
  const ok = async () => ({ ok: true });
  window.pywebview = {
    api: {
      status: async () => status(),
      meetings: async (q) => meetings.filter((m) => !q || (m.title + m.preview).toLowerCase().includes(q.toLowerCase())),
      meeting: async (id) => detail(id),
      start_recording: ok, stop_recording: ok, rename: ok, save_notulen: ok, retry: ok, delete: ok,
      download: ok, export: ok, copy_text: ok,
      save_settings: async (v) => v,
      import_audio: async () => ({ ok: false, error: "" }),
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
