// Browser-only stand-in for window.pywebview.api, for UI previews and
// screenshots without Windows. Loaded by scripts/preview.py, never shipped.
(() => {
  const params = new URLSearchParams(location.search);
  const scenario = params.get("s") || "home";
  const now = new Date();
  const iso = (minsAgo) => new Date(now - minsAgo * 60000).toISOString();
  const notulen = {
    title: "Sinkronisasi rilis aplikasi v2.3",
    summary: "Tim sepakat rilis v2.3 dimundurkan ke Jumat karena bug login di Android belum selesai. QA butuh dua hari untuk regression test setelah fix masuk. Marketing tetap menyiapkan materi launch, tapi jadwal posting digeser mengikuti tanggal rilis.",
    topics: ["Status bug login Android dan estimasi fix", "Kapasitas QA untuk regression test", "Penyesuaian jadwal materi launch"],
    decisions: ["Rilis v2.3 dimundurkan ke Jumat, 16 Oktober", "Fitur dark mode ditunda ke v2.4"],
    action_items: [
      { task: "Fix bug login Android dan push ke staging", owner: "Budi", due: "Rabu" },
      { task: "Regression test penuh setelah fix masuk", owner: "Sari", due: "Kamis" },
      { task: "Update jadwal posting launch di content calendar", owner: "Dewi", due: "" },
    ],
    open_questions: ["Apakah perlu hotfix untuk pengguna iOS versi lama?"],
  };
  const transcript = [
    { speaker: "Peserta", start: 2, end: 9, text: "Oke, kita mulai ya. Jadi update dari tim mobile gimana, Bud?" },
    { speaker: "Peserta", start: 10, end: 24, text: "Bug login di Android masih ada, root cause-nya udah ketemu, di token refresh. Fix-nya kira-kira selesai Rabu." },
    { speaker: "Saya", start: 25, end: 33, text: "Berarti QA baru bisa mulai Kamis ya. Sari, regression test butuh berapa lama?" },
    { speaker: "Peserta", start: 34, end: 44, text: "Minimal dua hari kalau full. Jadi paling cepat rilis Jumat." },
    { speaker: "Saya", start: 45, end: 56, text: "Oke, kita sepakat rilis mundur ke Jumat. Dark mode kita pindah ke v2.4 aja biar nggak nambah risiko." },
  ];
  const meetings = [
    { id: "m1", title: notulen.title, created_at: iso(35), duration: 2860, status: "done", source: "record", preview: notulen.summary, action_count: 3 },
    { id: "m2", title: "1:1 dengan Sari", created_at: iso(60 * 26), duration: 1710, status: "done", source: "record", preview: "Membahas rencana pengembangan karier Sari dan target Q4, termasuk pelatihan automation testing.", action_count: 2 },
    { id: "m3", title: "Rapat 9 Okt 2026, 13:00", created_at: iso(4), duration: 1240, status: "summarizing", source: "record", preview: "", action_count: 0 },
    { id: "m4", title: "Review budget Q4", created_at: iso(60 * 24 * 4), duration: 3520, status: "done", source: "import", preview: "Anggaran pemasaran Q4 disetujui dengan pemotongan 10% untuk iklan berbayar.", action_count: 4 },
  ];
  const assets = {
    "whisper-turbo": { label: "Model transkripsi (Whisper large-v3-turbo)", size: 1622e6, installed: scenario !== "setup" },
    "whisper-small": { label: "Model transkripsi ringan (Whisper small)", size: 486e6, installed: false },
    llm: { label: "Model notulen (Qwen3 4B Instruct)", size: 2497e6, installed: scenario !== "setup" },
    llama: { label: "Mesin AI lokal (llama.cpp)", size: 33e6, installed: scenario !== "setup" },
    cuda: { label: "Akselerasi GPU NVIDIA (cuBLAS + cuDNN)", size: 1260e6, installed: false },
  };
  const t0 = Date.now();
  const status = () => {
    const live = scenario === "recording";
    const el = (Date.now() - t0) / 1000 + 754;
    return {
      recording: live, recording_id: live ? "m3" : "", elapsed: live ? el : 0,
      levels: live ? { mic: 0.25 + 0.2 * Math.abs(Math.sin(el * 3)), system: 0.4 + 0.3 * Math.abs(Math.sin(el * 2.1)) } : {},
      devices: live ? { mic: "Microphone Array (Realtek)", system: "Headphones (WH-1000XM4)" } : {},
      activity: { meeting_id: scenario === "processing" ? "m3" : "", stage: "summarizing", progress: 0.42,
        downloads: scenario === "setup" ? { "whisper-turbo": { label: "", done: 0.37 * 1622e6, total: 1622e6, error: "" } } : {} },
      queue: 0,
      compute: { label: "GPU NVIDIA (RTX 3050 Ti Laptop GPU)", device: "cuda", gpu: "RTX 3050 Ti", cuda_ready: true },
      assets, needed: ["whisper-turbo", "llm", "llama", "cuda"], ready: scenario !== "setup",
      settings: { language: "id", notulen_language: "auto", whisper_model: "auto", glossary: ["Budi", "Sari", "Dewi", "regression test", "staging"], theme: params.get("theme") || "light" },
    };
  };
  const detail = (id) => {
    const m = meetings.find((x) => x.id === id);
    const done = m.status === "done";
    const labels = { summary: "Ringkasan", topics: "Pembahasan", decisions: "Keputusan", action_items: "Action items",
      open_questions: "Pertanyaan terbuka", none: "(tidak ada)", task: "Tugas", owner: "PIC", due: "Deadline" };
    return { ...m, language: "id", labels, error: "", transcript: done ? transcript : [], notulen: done ? notulen : null,
      timings: done ? { transcribe_s: 84, summarize_s: 71 } : {}, whatsapp: "", markdown: "" };
  };
  const api = {
    status: async () => status(),
    meetings: async (q) => meetings.filter((m) => !q || (m.title + m.preview).toLowerCase().includes(q.toLowerCase())),
    meeting: async (id) => detail(id),
    start_recording: async () => ({ ok: true }), stop_recording: async () => ({ ok: true }),
    rename: async () => ({ ok: true }), save_notulen: async () => ({ ok: true }), retry: async () => ({ ok: true }),
    delete: async () => ({ ok: true }), save_settings: async (v) => v, download: async () => ({ ok: true }),
    import_audio: async () => ({ ok: false, error: "" }), export: async () => ({ ok: true }), copy_text: async () => ({ ok: true }),
  };
  window.pywebview = { api };
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
