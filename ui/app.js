// Lariksa UI. Talks to the Rust app through window.pywebview.api (see desktop/src/bridge.js).
// All user and model text goes through textContent, never innerHTML.
"use strict";

const $ = (id) => document.getElementById(id);
const api = () => window.pywebview.api;
const state = { view: "home", current: null, meetings: [], status: null, search: "", dirty: false };

const BADGE_KIND = { recording: "busy", queued: "busy", transcribing: "busy", summarizing: "busy", done: "ok", failed: "fail" };
const statusLabel = (status) => (BADGE_KIND[status] ? t(`status.${status}`) : status);
const stageLabel = (stage) => (["queued", "transcribing", "summarizing"].includes(stage) ? t(`stage.${stage}`) : "");

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

/** An icon from the sprite in index.html. */
function icon(name) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

function toast(text) {
  const t = $("toast");
  t.textContent = text;
  t.classList.remove("hidden");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.add("hidden"), 2600);
}

const months = () => t("months").split(",");
function when(iso) {
  const d = new Date(iso);
  const today = new Date();
  const time = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (d.toDateString() === today.toDateString()) return t("date.today", { time });
  const y = new Date(today); y.setDate(today.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return t("date.yesterday", { time });
  return `${d.getDate()} ${months()[d.getMonth()]} ${d.getFullYear()}, ${time}`;
}
function duration(s) {
  s = Math.round(s || 0);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
}
function clock(s) { return duration(s); }
function bytes(n) { return n > 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.round(n / 1e6)} MB`; }
function badge(status) {
  return el("span", { class: `badge ${BADGE_KIND[status] || ""}` }, statusLabel(status));
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
  $("greeting").textContent = t(h < 11 ? "greeting.morning" : h < 15 ? "greeting.day" : h < 19 ? "greeting.afternoon" : "greeting.evening");
  const d = new Date();
  $("greeting-eyebrow").textContent = `${t("days").split(",")[d.getDay()]}, ${d.getDate()} ${months()[d.getMonth()].toUpperCase()}`;
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
      el("small", {}, `${when(m.created_at)}${m.status !== "done" ? " · " + statusLabel(m.status) : ""}`))));
}

function renderCards() {
  const cards = $("cards");
  const recent = state.meetings.slice(0, 6);
  if (!recent.length) {
    cards.replaceChildren(el("div", { class: "empty" },
      t(state.search ? "empty.search" : "empty.none")));
    return;
  }
  cards.replaceChildren(...recent.map((m) =>
    el("div", { class: "card", tabindex: 0, role: "button", onclick: () => openMeeting(m.id),
      onkeydown: (e) => e.key === "Enter" && openMeeting(m.id) },
      m.status === "done" ? null : badge(m.status),
      el("strong", {}, m.title),
      el("p", {}, m.preview || t(m.status === "failed" ? "card.failed" : "card.pending")),
      el("small", {}, `${when(m.created_at)} · ${duration(m.duration)}${m.action_count ? " · " + t("card.actions", { n: m.action_count }) : ""}`))));
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
  $("m-meta").textContent = `${when(m.created_at)} · ${duration(m.duration)} · ${t(m.source === "import" ? "meta.import" : "meta.record")}` +
    (m.timings.transcribe_s ? " · " + t("meta.transcribe", { n: Math.round(m.timings.transcribe_s) }) : "") +
    (m.timings.summarize_s ? " · " + t("meta.summarize", { n: Math.round(m.timings.summarize_s) }) : "");

  const busy = ["recording", "queued", "transcribing", "summarizing"].includes(m.status);
  $("m-progress").classList.toggle("hidden", !busy);
  ["copy-wa", "export-docx"].forEach((b) => ($(b).disabled = !m.notulen));
  $("delete").disabled = busy;
  updateProgress();

  const err = $("m-error");
  if (m.status === "failed") {
    err.replaceChildren(el("div", {}, m.error || t("failed.default")),
      el("button", { onclick: () => retry(false) }, t("retry")),
      m.transcript.length ? el("button", { class: "ghost", style: "margin-left:8px", onclick: () => retry(true) }, t("retry.summary")) : null);
    err.classList.remove("hidden");
  } else err.classList.add("hidden");

  if (resetTab) { selectTab("notulen"); $("m-find").value = ""; }
  if (!state.dirty) renderNotulen(m);
  renderTranscript(m);
  renderSide(m);
  $("regen").disabled = busy || !m.transcript.length;
  applyFind();
}

function updateProgress() {
  const m = state.meeting, s = state.status;
  if (!m || !s) return;
  const bar = $("m-progress");
  let label = statusLabel(m.status);
  let value = 0;
  if (s.activity.meeting_id === m.id) {
    label = stageLabel(s.activity.stage) || label;
    value = s.activity.progress;
  } else if (m.status === "queued") label = s.activity.meeting_id ? t("progress.waiting.other") : statusLabel("queued");
  else if (m.status === "recording") label = t("progress.recording");
  bar.querySelector("b").style.width = `${Math.round(value * 100)}%`;
  bar.querySelector("span").textContent = value ? `${label} · ${Math.round(value * 100)}%` : label;
}

function editable(tag, text, attrs = {}) {
  return el(tag, { ...attrs, class: `editable ${attrs.class || ""}`, contenteditable: "true", spellcheck: "false",
    oninput: () => { state.dirty = true; } }, text);
}

// ---------------------------------------------------------------- people
const AVATAR_COLORS = ["#C9F27A", "#DDE9CF", "#BFE3D0", "#F2E2B8", "#D8E4F2", "#EAD6E6", "#E2DCCF"];
function initials(name) {
  const parts = (name || "?").replace(/[^\p{L}\p{N} ]/gu, " ").trim().split(/\s+/).filter(Boolean);
  return ((parts[0]?.[0] || "?") + (parts.length > 1 ? parts[parts.length - 1][0] : "")).toUpperCase();
}
function colorFor(name) {
  let h = 0;
  for (const c of name || "") h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AVATAR_COLORS[h % AVATAR_COLORS.length];
}
function avatar(name) {
  return el("span", { class: "avatar", style: `background:${colorFor(name)}`, "aria-hidden": "true" }, initials(name));
}

// ---------------------------------------------------------------- time anchors
function tsChip(seconds, refs) {
  // Items without an anchor (notulen from the old format) get no chip: "00:00" would be a lie.
  if (!refs || !refs.length) return null;
  return el("button", { class: "ts", title: t("jump.title"),
    onclick: (e) => { e.stopPropagation(); jumpTo(seconds, refs[0]); } }, clock(seconds));
}

function jumpTo(seconds, lineNo) {
  selectTab("transcript");
  const row = document.querySelector(`#tab-transcript [data-line="${lineNo}"]`) ||
    [...document.querySelectorAll("#tab-transcript [data-start]")].reverse().find((r) => +r.dataset.start <= seconds + 0.5);
  if (row) {
    row.scrollIntoView({ block: "center", behavior: "smooth" });
    row.classList.remove("flash"); void row.offsetWidth; row.classList.add("flash");
  }
  const audio = $("audio");
  if (state.meeting?.audio_url && audio.src) { audio.currentTime = Math.max(0, seconds - 1); audio.play().catch(() => {}); }
}

// ---------------------------------------------------------------- notulen
function sec(title, count, ...children) {
  return el("section", { class: "sec" },
    el("div", { class: "sec-head" }, el("h3", {}, title), count === null ? null : el("span", { class: "count" }, String(count))),
    ...children);
}

function renderNotulen(m) {
  const body = $("tab-notulen");
  const n = m.notulen;
  if (!n) {
    body.replaceChildren(el("p", { class: "lede" }, t(m.status === "failed" ? "notulen.unavailable" : "notulen.pending")));
    return;
  }
  const L = m.labels;
  const legacy = n.version < 2;

  // Action items grouped by owner, in order of first appearance.
  const groups = new Map();
  n.actions.forEach((a, i) => {
    const key = a.owner || "";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push([a, i]);
  });
  const actionBlocks = [...groups.entries()].map(([owner, items]) => el("div", { class: "owner-group" },
    el("div", { class: "owner-head" }, avatar(owner || "?"), el("b", {}, owner || L.unassigned),
      el("small", {}, `${items.filter(([a]) => a.done).length}/${items.length}`)),
    ...items.map(([a, i]) => el("div", { class: `action${a.done ? " done" : ""}`, "data-i": i },
      el("input", { type: "checkbox", checked: a.done, "aria-label": t("action.done.aria"),
        onchange: (e) => { a.done = e.target.checked; e.target.closest(".action").classList.toggle("done", a.done); state.dirty = true; saveNotulen(); } }),
      editable("div", a.task, { class: "task", "data-field": "task" }),
      el("div", { class: "side" }, a.due ? el("span", { class: "due" }, a.due) : null, tsChip(a.start, a.refs))))));

  const pointList = (items, kind) => el("ul", { class: `points ${kind}` }, items.map((p, i) => el("li", { "data-i": i },
    el("span", { class: "mark" }, icon(kind === "q" ? "question" : "check")),
    editable("span", p.text, { "data-field": "text" }), tsChip(p.start, p.refs))));

  body.replaceChildren(...[
    legacy ? el("div", { class: "legacy-note" }, t("notulen.legacy")) : null,
    sec(L.summary, null,
      editable("p", n.overview, { class: "overview", "data-key": "overview" }),
      n.highlights.length ? el("ul", { class: "highlights", "data-key": "highlights" }, n.highlights.map((h) => editable("li", h))) : null),
    sec(L.actions, n.actions.length, n.actions.length ? el("div", { id: "actions" }, actionBlocks) : el("p", { class: "muted" }, L.none)),
    n.decisions.length ? sec(L.decisions, n.decisions.length, Object.assign(pointList(n.decisions, "d"), { id: "decisions" })) : null,
    n.questions.length ? sec(L.questions, n.questions.length, Object.assign(pointList(n.questions, "q"), { id: "questions" })) : null,
    n.chapters.length ? sec(L.chapters, n.chapters.length, el("div", { id: "chapters" }, n.chapters.map((c, i) =>
      el("div", { class: "chapter", "data-i": i }, el("div", {}, tsChip(c.start, c.refs)),
        el("div", {}, editable("h4", c.title, { "data-field": "title" }), editable("p", c.summary, { "data-field": "summary" })))))) : null,
    el("p", { class: "hint" }, t("notulen.hint")),
  ].filter(Boolean));
}

function collectNotulen() {
  const n = state.meeting.notulen;
  const body = $("tab-notulen");
  const field = (row, name) => row.querySelector(`[data-field="${name}"]`)?.textContent.trim() ?? "";
  const actions = [...body.querySelectorAll("#actions .action")].map((row) => {
    const a = n.actions[+row.dataset.i];
    return { task: field(row, "task"), owner: a.owner, due: a.due, done: row.querySelector("input").checked };
  });
  // Keep stored order (the UI groups by owner; the server matches items by position).
  const ordered = n.actions.map((_, i) => actions[[...body.querySelectorAll("#actions .action")].findIndex((r) => +r.dataset.i === i)]).filter(Boolean);
  const points = (id) => [...body.querySelectorAll(`#${id} li`)].map((li) => ({ text: field(li, "text") }));
  return {
    title: n.title,
    overview: body.querySelector('[data-key="overview"]')?.textContent.trim() || "",
    highlights: [...body.querySelectorAll('[data-key="highlights"] li')].map((li) => li.textContent.trim()).filter(Boolean),
    actions: ordered,
    ...(body.querySelector("#decisions") ? { decisions: points("decisions") } : {}),
    ...(body.querySelector("#questions") ? { questions: points("questions") } : {}),
    ...(body.querySelector("#chapters") ? { chapters: [...body.querySelectorAll("#chapters .chapter")].map((c) => ({ title: field(c, "title"), summary: field(c, "summary") })) } : {}),
  };
}

async function saveNotulen() {
  if (!state.dirty || !state.meeting?.notulen) { state.dirty = false; return; }
  state.dirty = false;
  await api().save_notulen(state.meeting.id, collectNotulen());
  const fresh = await api().meeting(state.meeting.id);
  if (fresh) state.meeting = fresh;
}

// ---------------------------------------------------------------- side rail
function renderSide(m) {
  const n = m.notulen;
  const toc = $("side-chapters");
  toc.replaceChildren(...(n?.chapters?.length ? [el("h5", {}, m.labels.chapters),
    el("ul", { class: "toc" }, n.chapters.map((c) => el("li", {}, el("button", { class: "chapter-link", "data-start": c.start, onclick: () => jumpTo(c.start, c.refs[0]) },
      el("time", {}, clock(c.start)), el("span", {}, c.title)))))] : []));

  const spk = $("side-speakers");
  const stats = n?.speakers?.length ? n.speakers : [];
  spk.replaceChildren(...(stats.length > 1 ? [el("h5", {}, m.labels.speakers), el("div", { class: "spk" },
    stats.slice(0, 10).flatMap((s) => [el("div", {}, avatar(s.name), el("span", { class: "name", title: s.name }, s.name),
      el("span", { class: "pct" }, `${Math.round(s.share * 100)}%`), el("span", { class: "bar" }, el("b", { style: `width:${Math.max(2, s.share * 100)}%` })))]))] : []));

  const player = $("player"), audio = $("audio");
  player.classList.toggle("hidden", !m.audio_url);
  if (m.audio_url && audio.dataset.src !== m.audio_url) {
    audio.dataset.src = m.audio_url;
    audio.src = m.audio_url;
    const total = m.duration || 0;
    $("t-total").textContent = clock(total);
    $("scrub-marks").replaceChildren(...(n?.chapters || []).map((c) => el("i", { style: `left:${total ? (c.start / total) * 100 : 0}%`, title: c.title })));
  }
}

function wirePlayer() {
  const audio = $("audio");
  $("play").addEventListener("click", () => (audio.paused ? audio.play() : audio.pause()));
  audio.addEventListener("play", () => $("play-icon").setAttribute("href", "#i-pause"));
  audio.addEventListener("pause", () => $("play-icon").setAttribute("href", "#i-play"));
  audio.addEventListener("timeupdate", () => {
    const total = audio.duration || state.meeting?.duration || 0;
    $("t-now").textContent = clock(audio.currentTime);
    $("scrub-fill").style.width = `${total ? (audio.currentTime / total) * 100 : 0}%`;
    // Follow along: highlight the transcript turn and chapter being played.
    const rows = [...document.querySelectorAll("#tab-transcript [data-start]")];
    const now = rows.filter((r) => +r.dataset.start <= audio.currentTime).pop();
    rows.forEach((r) => r.classList.toggle("playing", r === now));
    const chapters = [...document.querySelectorAll(".toc button")];
    const ch = chapters.filter((b) => +b.dataset.start <= audio.currentTime).pop();
    chapters.forEach((b) => b.classList.toggle("now", b === ch));
  });
  $("scrub").addEventListener("click", (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    const total = audio.duration || state.meeting?.duration || 0;
    audio.currentTime = ((e.clientX - r.left) / r.width) * total;
    audio.play().catch(() => {});
  });
}

// ---------------------------------------------------------------- transcript
function renderTranscript(m) {
  const body = $("tab-transcript");
  if (!m.lines?.length) {
    body.replaceChildren(el("p", { class: "lede" }, t("transcript.pending")));
    return;
  }
  const chapters = (m.notulen?.chapters || []).slice().sort((a, b) => a.start - b.start);
  const out = [];
  let ci = 0;
  for (const line of m.lines) {
    while (ci < chapters.length && chapters[ci].refs.length && chapters[ci].refs[0] <= line.n) {
      out.push(el("div", { class: "chapter-break" }, el("time", {}, clock(chapters[ci].start)), chapters[ci].title));
      ci++;
    }
    out.push(el("div", { class: `turn ${line.speaker === "Saya" ? "me" : ""}`, "data-line": line.n, "data-start": line.start },
      el("time", { onclick: () => jumpTo(line.start, line.n) }, clock(line.start)),
      el("div", {}, el("div", { class: "who" }, line.speaker), el("p", {}, line.text))));
  }
  body.replaceChildren(...out);
}

// ---------------------------------------------------------------- find in meeting
function applyFind() {
  const q = $("m-find").value.trim().toLowerCase();
  document.querySelectorAll("#view-meeting mark").forEach((mk) => mk.replaceWith(document.createTextNode(mk.textContent)));
  document.querySelectorAll("#tab-transcript .turn, #tab-notulen .action, #tab-notulen .points li, #tab-notulen .chapter")
    .forEach((n) => (n.style.display = ""));
  if (!q) return;
  if (q.length < 2) return;
  let first = null;
  const scope = $("tab-transcript").classList.contains("hidden") ? $("tab-notulen") : $("tab-transcript");
  scope.querySelectorAll(".turn, .action, .points li, .chapter").forEach((node) => {
    const hit = node.textContent.toLowerCase().includes(q);
    node.style.display = hit ? "" : "none";
    if (hit) { highlight(node, q); first = first || node; }
  });
  first?.scrollIntoView({ block: "nearest" });
}

function highlight(node, q) {
  const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  const hits = [];
  while (walker.nextNode()) if (walker.currentNode.nodeValue.toLowerCase().includes(q)) hits.push(walker.currentNode);
  for (const t of hits) {
    const i = t.nodeValue.toLowerCase().indexOf(q);
    const mid = t.splitText(i);
    mid.splitText(q.length);
    const mk = document.createElement("mark");
    mk.textContent = mid.nodeValue;
    mid.replaceWith(mk);
  }
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
      else { toast(t("toast.saved.recording")); $("meeting-title").value = ""; await loadMeetings(); }
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
  $("record-label").textContent = t(live ? "record.stop" : "record.button");
  $("record-title").textContent = live ? t("record.live.title", { time: duration(s.elapsed) }) : t("record.start.title");
  $("record-sub").textContent = live
    ? t("record.live.sub", { mic: s.devices.mic || "-", system: s.devices.system || "-" })
    : t("record.start.sub");
  const consent = $("consent");
  const silent = live ? s.silent || [] : [];
  consent.classList.toggle("warn", silent.length > 0);
  consent.textContent = t(silent.includes("mic") ? "consent.mic" : silent.includes("system") ? "consent.system" : "consent");
  $("lvl-mic").style.width = `${Math.min(100, (s.levels.mic || 0) * 140)}%`;
  $("lvl-system").style.width = `${Math.min(100, (s.levels.system || 0) * 140)}%`;
  $("record").disabled = !s.ready && !live;
  const hint = $("kbd-hint");
  hint.classList.toggle("hidden", !state.hotkeyLabel);
  if (state.hotkeyLabel) {
    hint.replaceChildren(...state.hotkeyLabel.split("+").flatMap((k, i) => [i ? " + " : "", el("kbd", {}, k)]),
      t(live ? "record.hotkey.stop" : "record.hotkey.start"));
  }
  $("record").title = s.ready ? "" : t("record.notready");
}

// ---------------------------------------------------------------- setup + settings
function renderSetup(s) {
  const missing = s.needed.filter((k) => !s.assets[k].installed);
  const card = $("setup-card");
  card.classList.toggle("hidden", !missing.length);
  if (!missing.length) return;
  const dl = s.activity.downloads;
  $("setup-list").replaceChildren(...missing.map((k) => {
    const d = dl[k];
    const right = d ? (d.error ? t("setup.failed") : `${Math.round((d.done / d.total) * 100)}%`) : bytes(s.assets[k].size);
    return el("li", {}, el("span", {}, s.assets[k].label), el("span", {}, right));
  }));
  const downloading = Object.keys(dl).length && !Object.values(dl).some((d) => d.error);
  $("setup-go").disabled = !!downloading;
  $("setup-go").textContent = downloading ? t("setup.going") : t("setup.go", { size: bytes(missing.reduce((a, k) => a + s.assets[k].size, 0)) });
}

async function downloadAssets(keys) {
  const r = await api().download(keys);
  if (!r.ok) toast(r.error);
  poll();
}

async function removeAsset(k) {
  if (!confirm(t("confirm.remove", { name: state.status.assets[k].label }))) return;
  const r = await api().remove_asset(k);
  if (!r.ok) toast(r.error);
  await poll();
  renderAssets(state.status);
}

function renderAssets(s) {
  const box = $("assets");
  // The minutes model stays listed in API mode, so it can be removed or fetched again.
  const keys = [...new Set([...s.needed, "whisper-small", "llm"])];
  box.replaceChildren(...keys.map((k) => {
    const a = s.assets[k], d = s.activity.downloads[k];
    const unused = !s.needed.includes(k);
    let right;
    if (a.installed && unused && a.removable) right = el("button", { class: "ghost", onclick: () => removeAsset(k) }, t("asset.remove"));
    else if (a.installed) right = el("span", { class: "ok" }, t("asset.installed"));
    else if (d && !d.error) right = el("span", {}, `${Math.round((d.done / d.total) * 100)}%`);
    else right = el("button", { class: "ghost", onclick: () => downloadAssets([k]) }, t(d?.error ? "asset.retry" : "asset.download"));
    return el("div", { class: "asset" },
      el("div", {}, el("div", { class: "name" }, a.label),
        el("small", {}, unused && a.installed ? t("asset.unused", { size: bytes(a.size) }) : bytes(a.size)),
        d?.error ? el("div", { class: "err" }, d.error) : null),
      right);
  }));
}

function renderSettings() {
  const s = state.status;
  if (!s) return;
  $("s-ui-language").value = s.settings.ui_language || "id";
  $("s-language").value = s.settings.language;
  $("s-notulen-language").value = s.settings.notulen_language;
  $("s-whisper").value = s.settings.whisper_model;
  $("s-glossary").value = s.settings.glossary.join(", ");
  $("s-hotkey").value = s.settings.hotkey || "";
  $("s-pill").checked = s.settings.show_pill !== false;
  const provider = s.settings.llm_provider === "api" ? "api" : "local";
  document.querySelector(`input[name="s-llm"][value="${provider}"]`).checked = true;
  $("s-api-url").value = s.settings.llm_api_url || "";
  $("s-api-model").value = s.settings.llm_api_model || "";
  $("s-api-key").value = "";
  renderApiKeyNote(s.settings.llm_api_key_set);
  $("s-api-status").textContent = "";
  showApiFields();
  showHotkeyError(state.hotkeyError);
  renderAssets(s);
}

function llmProvider() {
  return document.querySelector('input[name="s-llm"]:checked')?.value || "local";
}

function showApiFields() {
  $("s-api").classList.toggle("hidden", llmProvider() !== "api");
}

// The saved key never comes back to the screen; an empty field keeps it.
function renderApiKeyNote(isSet) {
  $("s-api-key").placeholder = isSet ? "••••••••" : "sk-…";
  $("s-api-key-note").textContent = t(isSet ? "s.api.key.saved" : "s.api.key.none");
}

async function checkApi() {
  const status = $("s-api-status");
  status.classList.remove("err", "ok");
  status.textContent = t("s.api.checking");
  const r = await api().check_llm_api($("s-api-url").value, $("s-api-key").value);
  if (!r.ok) {
    status.classList.add("err");
    status.textContent = r.error;
    return;
  }
  $("s-api-models").replaceChildren(...r.models.map((m) => el("option", { value: m })));
  status.classList.add("ok");
  status.textContent = t("s.api.ok", { n: r.models.length });
}

async function saveSettings() {
  const values = {
    ui_language: $("s-ui-language").value, language: $("s-language").value, notulen_language: $("s-notulen-language").value,
    whisper_model: $("s-whisper").value, glossary: $("s-glossary").value,
    hotkey: $("s-hotkey").value.trim(), show_pill: $("s-pill").checked,
    llm_provider: llmProvider(), llm_api_url: $("s-api-url").value, llm_api_model: $("s-api-model").value,
  };
  if ($("s-api-key").value.trim()) values.llm_api_key = $("s-api-key").value;
  const saved = await api().save_settings(values);
  if (saved.ok === false) {
    toast(saved.error);
    return;
  }
  state.hotkeyError = saved.hotkey_error || "";
  state.hotkeyLabel = state.hotkeyError ? "" : (saved.hotkey || "");
  applyLanguage(saved.ui_language);
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
  $("engine-label").textContent = t("engine", { label: s.compute.label });
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
  document.querySelectorAll(".tab").forEach((b) => b.addEventListener("click", () => { selectTab(b.dataset.tab); applyFind(); }));
  $("back").addEventListener("click", async () => { await saveNotulen(); show("home"); });
  $("record").addEventListener("click", toggleRecord);
  $("setup-go").addEventListener("click", () => downloadAssets(state.status.needed.filter((k) => !state.status.assets[k].installed)));
  $("s-save").addEventListener("click", saveSettings);
  document.querySelectorAll('input[name="s-llm"]').forEach((r) => r.addEventListener("change", showApiFields));
  $("s-api-check").addEventListener("click", checkApi);
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
    else if (r?.ok) { toast(t("toast.imported")); loadMeetings(); }
  });
  $("m-title").addEventListener("change", async (e) => {
    const r = await api().rename(state.current, e.target.value);
    if (!r.ok) toast(r.error); else loadMeetings();
  });
  $("tab-notulen").addEventListener("focusout", () => saveNotulen());
  $("m-find").addEventListener("input", () => applyFind());
  $("regen").addEventListener("click", async () => {
    if (!confirm(t("confirm.regen"))) return;
    await retry(true);
  });
  wirePlayer();
  $("copy-wa").addEventListener("click", async () => {
    await saveNotulen();
    const m = await api().meeting(state.current);
    await api().copy_text(m.whatsapp);
    toast(t("toast.copied"));
  });
  $("export-docx").addEventListener("click", async () => { await saveNotulen(); const r = await api().export(state.current, "docx"); if (r?.ok) toast(t("toast.docx")); else if (r?.error) toast(r.error); });
  $("export-md").addEventListener("click", async () => { await saveNotulen(); const r = await api().export(state.current, "md"); if (r?.ok) toast(t("toast.md")); else if (r?.error) toast(r.error); });
  $("delete").addEventListener("click", async () => {
    if (!confirm(t("confirm.delete"))) return;
    const r = await api().delete(state.current);
    if (!r.ok) toast(r.error); else { toast(t("toast.deleted")); show("home"); loadMeetings(); }
  });
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); $("search").focus(); }
  });
}

function showHotkeyError(error) {
  const note = $("s-hotkey-note");
  note.classList.toggle("warn-text", !!error);
  note.textContent = error || t("s.hotkey.note");
}

/** Switch the display language and redraw everything drawn from code. */
function applyLanguage(lang) {
  if (!setUiLanguage(lang)) return;
  greet();
  if (state.status) { renderRecorder(state.status); renderSetup(state.status); $("engine-label").textContent = t("engine", { label: state.status.compute.label }); }
  renderList(); renderCards();
  if (state.view === "settings") renderSettings();
  if (state.view === "meeting" && state.current) renderMeeting();
}

async function boot() {
  wire();
  greet();
  state.hotkeyLabel = await api().hotkey_label();
  state.hotkeyError = await api().hotkey_error();
  const s = await api().status();
  document.documentElement.dataset.theme = s.settings.theme || "light";
  applyLanguage(s.settings.ui_language);
  await poll();
  setInterval(poll, 500);
}

window.addEventListener("pywebviewready", boot);
