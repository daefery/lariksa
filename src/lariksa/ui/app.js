// Lariksa UI. Talks to Python through window.pywebview.api (see bridge.py).
// All user and model text goes through textContent, never innerHTML.
"use strict";

const $ = (id) => document.getElementById(id);
const api = () => window.pywebview.api;
const state = { view: "home", current: null, meetings: [], status: null, search: "", dirty: false };

const STAGES = { queued: "Menunggu giliran", transcribing: "Mentranskripsi", summarizing: "Menyusun notulen" };
const STATUS_BADGE = {
  recording: ["busy", "Merekam"], queued: ["busy", "Menunggu"], transcribing: ["busy", "Transkripsi"],
  summarizing: ["busy", "Menyusun notulen"], done: ["ok", "Selesai"], failed: ["fail", "Gagal"],
};

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (v !== undefined && v !== null && v !== false) node.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) if (c !== null && c !== undefined) node.append(c);
  return node;
}

function toast(text) {
  const t = $("toast");
  t.textContent = text;
  t.classList.remove("hidden");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.add("hidden"), 2600);
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
function when(iso) {
  const d = new Date(iso);
  const today = new Date();
  const time = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (d.toDateString() === today.toDateString()) return `Hari ini, ${time}`;
  const y = new Date(today); y.setDate(today.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return `Kemarin, ${time}`;
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()}, ${time}`;
}
function duration(s) {
  s = Math.round(s || 0);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
}
function clock(s) { return duration(s); }
function bytes(n) { return n > 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.round(n / 1e6)} MB`; }
function badge(status) {
  const [kind, label] = STATUS_BADGE[status] || ["", status];
  return el("span", { class: `badge ${kind}` }, label);
}

// ---------------------------------------------------------------- views
function show(view) {
  state.view = view;
  for (const v of ["home", "meeting", "settings"]) $(`view-${v}`).classList.toggle("hidden", v !== view);
  document.querySelectorAll(".nav").forEach((b) => b.classList.toggle("active", b.dataset.view === view));
  if (view === "settings") renderSettings();
  if (view !== "meeting") { state.current = null; renderList(); }
}

function greet() {
  const h = new Date().getHours();
  $("greeting").textContent = h < 11 ? "Selamat pagi." : h < 15 ? "Selamat siang." : h < 19 ? "Selamat sore." : "Selamat malam.";
  const d = new Date();
  $("greeting-eyebrow").textContent = `${["MINGGU", "SENIN", "SELASA", "RABU", "KAMIS", "JUMAT", "SABTU"][d.getDay()]}, ${d.getDate()} ${MONTHS[d.getMonth()].toUpperCase()}`;
}

// ---------------------------------------------------------------- list + cards
async function loadMeetings() {
  state.meetings = await api().meetings(state.search);
  renderList();
  renderCards();
}

function renderList() {
  const list = $("meeting-list");
  list.replaceChildren(...state.meetings.map((m) =>
    el("li", { class: state.current === m.id ? "active" : "", onclick: () => openMeeting(m.id), title: m.title },
      el("span", { class: "t" }, m.title),
      el("small", {}, `${when(m.created_at)}${m.status !== "done" ? " · " + (STATUS_BADGE[m.status] || [, m.status])[1] : ""}`))));
}

function renderCards() {
  const cards = $("cards");
  const recent = state.meetings.slice(0, 6);
  if (!recent.length) {
    cards.replaceChildren(el("div", { class: "empty" },
      state.search ? "Tidak ada rapat yang cocok." : "Halaman baru untuk rapat berikutnya. Tekan Rekam saat rapat dimulai."));
    return;
  }
  cards.replaceChildren(...recent.map((m) =>
    el("div", { class: "card", tabindex: 0, role: "button", onclick: () => openMeeting(m.id),
      onkeydown: (e) => e.key === "Enter" && openMeeting(m.id) },
      m.status === "done" ? null : badge(m.status),
      el("strong", {}, m.title),
      el("p", {}, m.preview || (m.status === "failed" ? "Proses gagal. Buka untuk mencoba lagi." : "Notulen sedang disiapkan…")),
      el("small", {}, `${when(m.created_at)} · ${duration(m.duration)}${m.action_count ? ` · ${m.action_count} action item` : ""}`))));
}

// ---------------------------------------------------------------- meeting detail
async function openMeeting(id) {
  if (state.dirty) await saveNotulen();
  state.current = id;
  show("meeting");
  state.current = id;
  renderList();
  await renderMeeting(true);
}

async function renderMeeting(resetTab = false) {
  const m = await api().meeting(state.current);
  if (!m) { show("home"); return; }
  state.meeting = m;
  if (document.activeElement !== $("m-title")) $("m-title").value = m.title;
  $("m-meta").textContent = `${when(m.created_at)} · ${duration(m.duration)} · ${m.source === "import" ? "Impor" : "Rekaman"}` +
    (m.timings.transcribe_s ? ` · transkripsi ${Math.round(m.timings.transcribe_s)} dtk` : "") +
    (m.timings.summarize_s ? ` · notulen ${Math.round(m.timings.summarize_s)} dtk` : "");

  const busy = ["recording", "queued", "transcribing", "summarizing"].includes(m.status);
  $("m-progress").classList.toggle("hidden", !busy);
  ["copy-wa", "export-docx"].forEach((b) => ($(b).disabled = !m.notulen));
  $("delete").disabled = busy;
  updateProgress();

  const err = $("m-error");
  if (m.status === "failed") {
    err.replaceChildren(el("div", {}, m.error || "Proses gagal."),
      el("button", { onclick: () => retry(false) }, "Coba lagi"),
      m.transcript.length ? el("button", { class: "ghost", style: "margin-left:8px", onclick: () => retry(true) }, "Ulangi notulen saja") : null);
    err.classList.remove("hidden");
  } else err.classList.add("hidden");

  if (resetTab) selectTab("notulen");
  if (!state.dirty) renderNotulen(m);
  renderTranscript(m);
}

function updateProgress() {
  const m = state.meeting, s = state.status;
  if (!m || !s) return;
  const bar = $("m-progress");
  let label = STATUS_BADGE[m.status]?.[1] || "";
  let value = 0;
  if (s.activity.meeting_id === m.id) {
    label = STAGES[s.activity.stage] || label;
    value = s.activity.progress;
  } else if (m.status === "queued") label = s.activity.meeting_id ? "Menunggu rapat lain selesai diproses" : "Menunggu";
  else if (m.status === "recording") label = "Sedang merekam";
  bar.querySelector("b").style.width = `${Math.round(value * 100)}%`;
  bar.querySelector("span").textContent = value ? `${label} · ${Math.round(value * 100)}%` : label;
}

function editable(tag, text, onChange, attrs = {}) {
  return el(tag, { ...attrs, class: `editable ${attrs.class || ""}`, contenteditable: "true", spellcheck: "false",
    oninput: () => { state.dirty = true; onChange(); } }, text);
}

function renderNotulen(m) {
  const body = $("tab-notulen");
  const n = m.notulen;
  if (!n) {
    body.replaceChildren(el("p", { class: "lede" }, m.status === "failed" ? "Notulen belum tersedia." : "Notulen akan muncul di sini setelah rapat diproses."));
    return;
  }
  // Headings come from the server (notulen.LABELS) in the notulen's own language.
  const t = m.labels;
  const list = (key, items, empty) => items.length
    ? el("ul", { "data-key": key }, items.map((x) => editable("li", x, () => {})))
    : el("p", { class: "muted", "data-key": key }, empty);

  const rows = n.action_items.map((a) => el("tr", {},
    editable("td", a.task, () => {}), editable("td", a.owner || "", () => {}, { class: a.owner ? "" : "muted" }),
    editable("td", a.due || "", () => {}, { class: a.due ? "" : "muted" })));

  body.replaceChildren(
    el("h3", {}, t.summary), editable("p", n.summary, () => {}, { class: "summary", "data-key": "summary" }),
    el("h3", {}, t.decisions), list("decisions", n.decisions, t.none),
    el("h3", {}, t.action_items),
    n.action_items.length
      ? el("table", { class: "actions-table", id: "actions" },
          el("thead", {}, el("tr", {}, el("th", {}, t.task.toUpperCase()), el("th", {}, t.owner.toUpperCase()), el("th", {}, t.due.toUpperCase()))),
          el("tbody", {}, rows))
      : el("p", { class: "muted" }, t.none),
    el("h3", {}, t.topics), list("topics", n.topics, "-"),
    el("h3", {}, t.open_questions), list("open_questions", n.open_questions, t.none),
    el("p", { class: "hint" }, "Klik teks mana pun untuk mengedit. Perubahan tersimpan otomatis."));
}

function collectNotulen() {
  const body = $("tab-notulen");
  const items = (key) => [...body.querySelectorAll(`ul[data-key="${key}"] li`)].map((li) => li.textContent.trim()).filter(Boolean);
  const actions = [...body.querySelectorAll("#actions tbody tr")].map((tr) => {
    const [task, owner, due] = [...tr.children].map((td) => td.textContent.trim());
    return { task, owner, due };
  }).filter((a) => a.task);
  return {
    title: state.meeting.notulen.title,
    summary: body.querySelector('[data-key="summary"]')?.textContent.trim() || "",
    decisions: items("decisions"), topics: items("topics"), open_questions: items("open_questions"),
    action_items: actions,
  };
}

async function saveNotulen() {
  if (!state.dirty || !state.meeting?.notulen) { state.dirty = false; return; }
  state.dirty = false;
  await api().save_notulen(state.meeting.id, collectNotulen());
  const fresh = await api().meeting(state.meeting.id);
  if (fresh) state.meeting = fresh;
}

function renderTranscript(m) {
  const body = $("tab-transcript");
  if (!m.transcript.length) {
    body.replaceChildren(el("p", { class: "lede" }, "Transkrip akan muncul di sini."));
    return;
  }
  const turns = [];
  for (const u of m.transcript) {
    const last = turns[turns.length - 1];
    if (last && last.speaker === u.speaker && u.start - last.end < 4) { last.text += " " + u.text; last.end = u.end; }
    else turns.push({ ...u });
  }
  body.replaceChildren(...turns.map((t) => el("div", { class: `turn ${t.speaker === "Saya" ? "me" : ""}` },
    el("time", {}, clock(t.start)), el("div", {}, el("div", { class: "who" }, t.speaker), el("p", {}, t.text)))));
}

function selectTab(tab) {
  document.querySelectorAll(".tab").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab));
  $("tab-notulen").classList.toggle("hidden", tab !== "notulen");
  $("tab-transcript").classList.toggle("hidden", tab !== "transcript");
}

async function retry(summaryOnly) {
  const r = await api().retry(state.current, summaryOnly);
  if (!r.ok) toast(r.error); else renderMeeting();
}

// ---------------------------------------------------------------- recording
async function toggleRecord() {
  const btn = $("record");
  btn.disabled = true;
  try {
    if (state.status?.recording) {
      const r = await api().stop_recording();
      if (!r.ok) toast(r.error);
      else { toast("Rekaman disimpan. Notulen sedang disiapkan."); $("meeting-title").value = ""; await loadMeetings(); }
    } else {
      const r = await api().start_recording($("meeting-title").value);
      if (!r.ok) toast(r.error);
      else await loadMeetings();
    }
    await poll();
  } finally { btn.disabled = false; }
}

function renderRecorder(s) {
  const live = s.recording;
  $("record-card").classList.toggle("live", live);
  $("meters").classList.toggle("hidden", !live);
  $("record-label").textContent = live ? "Selesai" : "Rekam";
  $("record-title").textContent = live ? `Merekam · ${duration(s.elapsed)}` : "Mulai rapat";
  $("record-sub").textContent = live
    ? `Mic: ${s.devices.mic || "-"} · Audio rapat: ${s.devices.system || "-"}`
    : "Rekam suaramu dan suara peserta dari Meet, Zoom, atau Teams.";
  const consent = $("consent");
  const silent = live ? s.silent || [] : [];
  consent.classList.toggle("warn", silent.length > 0);
  consent.textContent = silent.includes("mic")
    ? "Mic tidak menangkap suara. Cek tombol mute di laptop atau di Zoom/Meet."
    : silent.includes("system")
      ? "Belum ada suara peserta. Pastikan audio rapat keluar dari speaker/headset default."
      : "Beri tahu peserta bahwa rapat ini direkam.";
  $("lvl-mic").style.width = `${Math.min(100, (s.levels.mic || 0) * 140)}%`;
  $("lvl-system").style.width = `${Math.min(100, (s.levels.system || 0) * 140)}%`;
  $("record").disabled = !s.ready && !live;
  $("record").title = s.ready ? "" : "Unduh model AI dulu";
}

// ---------------------------------------------------------------- setup + settings
function renderSetup(s) {
  // CUDA is optional, but offered up front: it is the difference between ~3 min
  // and ~20 s to transcribe 10 minutes of audio.
  const missing = s.needed.filter((k) => !s.assets[k].installed);
  const card = $("setup-card");
  card.classList.toggle("hidden", !missing.length);
  if (!missing.length) return;
  const dl = s.activity.downloads;
  $("setup-list").replaceChildren(...missing.map((k) => {
    const d = dl[k];
    const right = d ? (d.error ? "Gagal" : `${Math.round((d.done / d.total) * 100)}%`) : bytes(s.assets[k].size);
    return el("li", {}, el("span", {}, s.assets[k].label), el("span", {}, right));
  }));
  const downloading = Object.keys(dl).length && !Object.values(dl).some((d) => d.error);
  $("setup-go").disabled = !!downloading;
  $("setup-go").textContent = downloading ? "Mengunduh…" : `Unduh semua (${bytes(missing.reduce((a, k) => a + s.assets[k].size, 0))})`;
}

async function downloadAssets(keys) {
  const r = await api().download(keys);
  if (!r.ok) toast(r.error);
  poll();
}

function renderAssets(s) {
  const box = $("assets");
  const keys = [...new Set([...s.needed, "whisper-small"])];
  box.replaceChildren(...keys.map((k) => {
    const a = s.assets[k], d = s.activity.downloads[k];
    let right;
    if (a.installed) right = el("span", { class: "ok" }, "Terpasang");
    else if (d && !d.error) right = el("span", {}, `${Math.round((d.done / d.total) * 100)}%`);
    else right = el("button", { class: "ghost", onclick: () => downloadAssets([k]) }, d?.error ? "Coba lagi" : "Unduh");
    const note = k === "cuda" ? "Opsional. Transkripsi 5-10x lebih cepat di GPU NVIDIA." : "";
    return el("div", { class: "asset" },
      el("div", {}, el("div", { class: "name" }, a.label),
        el("small", {}, `${bytes(a.size)}${note ? " · " + note : ""}`),
        d?.error ? el("div", { class: "err" }, d.error) : null),
      right);
  }));
}

function renderSettings() {
  const s = state.status;
  if (!s) return;
  $("s-language").value = s.settings.language;
  $("s-notulen-language").value = s.settings.notulen_language;
  $("s-whisper").value = s.settings.whisper_model;
  $("s-glossary").value = s.settings.glossary.join(", ");
  renderAssets(s);
}

async function saveSettings() {
  await api().save_settings({
    language: $("s-language").value, notulen_language: $("s-notulen-language").value,
    whisper_model: $("s-whisper").value, glossary: $("s-glossary").value,
  });
  $("s-saved").classList.remove("hidden");
  setTimeout(() => $("s-saved").classList.add("hidden"), 2000);
  await poll();
  renderSettings();
}

// ---------------------------------------------------------------- polling
let lastSignature = "";
async function poll() {
  const s = await api().status();
  state.status = s;
  renderRecorder(s);
  renderSetup(s);
  $("engine-label").textContent = `Mesin: ${s.compute.label}`;
  if (state.view === "settings" && document.activeElement?.tagName !== "TEXTAREA") renderAssets(s);
  if (state.view === "meeting") updateProgress();

  // Refresh lists when anything about the job pipeline changed.
  const signature = JSON.stringify([s.activity.meeting_id, s.activity.stage, s.queue, s.recording]);
  if (signature !== lastSignature) {
    lastSignature = signature;
    await loadMeetings();
    if (state.view === "meeting" && state.current) await renderMeeting();
  }
}

// ---------------------------------------------------------------- wiring
function wire() {
  document.querySelectorAll(".nav").forEach((b) => b.addEventListener("click", async () => { await saveNotulen(); show(b.dataset.view); }));
  document.querySelectorAll(".tab").forEach((b) => b.addEventListener("click", () => selectTab(b.dataset.tab)));
  $("back").addEventListener("click", async () => { await saveNotulen(); show("home"); });
  $("record").addEventListener("click", toggleRecord);
  $("setup-go").addEventListener("click", () => downloadAssets(state.status.needed.filter((k) => !state.status.assets[k].installed)));
  $("s-save").addEventListener("click", saveSettings);
  $("theme").addEventListener("click", async () => {
    const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    await api().save_settings({ theme: next });
  });
  let searchTimer;
  $("search").addEventListener("input", (e) => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { state.search = e.target.value; loadMeetings(); }, 180);
  });
  $("import").addEventListener("click", async () => {
    const r = await api().import_audio();
    if (r && !r.ok && r.error) toast(r.error);
    else if (r?.ok) { toast("Rekaman diimpor. Notulen sedang disiapkan."); loadMeetings(); }
  });
  $("m-title").addEventListener("change", async (e) => {
    const r = await api().rename(state.current, e.target.value);
    if (!r.ok) toast(r.error); else loadMeetings();
  });
  $("tab-notulen").addEventListener("focusout", () => saveNotulen());
  $("copy-wa").addEventListener("click", async () => {
    await saveNotulen();
    const m = await api().meeting(state.current);
    await api().copy_text(m.whatsapp);
    toast("Disalin. Tempel di WhatsApp.");
  });
  $("export-docx").addEventListener("click", async () => { await saveNotulen(); const r = await api().export(state.current, "docx"); if (r?.ok) toast("DOCX tersimpan."); else if (r?.error) toast(r.error); });
  $("export-md").addEventListener("click", async () => { await saveNotulen(); const r = await api().export(state.current, "md"); if (r?.ok) toast("Markdown tersimpan."); else if (r?.error) toast(r.error); });
  $("delete").addEventListener("click", async () => {
    if (!confirm("Hapus rapat ini beserta rekaman audionya? Tindakan ini tidak bisa dibatalkan.")) return;
    const r = await api().delete(state.current);
    if (!r.ok) toast(r.error); else { toast("Rapat dihapus."); show("home"); loadMeetings(); }
  });
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); $("search").focus(); }
  });
}

async function boot() {
  wire();
  greet();
  const s = await api().status();
  document.documentElement.dataset.theme = s.settings.theme || "light";
  await poll();
  setInterval(poll, 500);
}

window.addEventListener("pywebviewready", boot);
