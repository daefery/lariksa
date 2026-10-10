// Every piece of on-screen copy, in Indonesian and English.
//
// Static HTML marks text with data-i18n="key" (textContent), data-i18n-placeholder,
// data-i18n-title and data-i18n-aria; scripts call t("key", {name: value}).
// The language comes from the "ui_language" setting and switches without a restart.
// Notulen headings are not here: they follow the language of the notulen itself.
"use strict";

const I18N = {
  id: {
    // navigation
    "nav.home": "Beranda", "nav.settings": "Pengaturan", "nav.meetings": "RAPAT",
    "search.placeholder": "Cari catatanmu", "search.aria": "Daftar rapat",
    "engine": "Mesin: {label}",
    // home
    "theme": "Tema", "theme.aria": "Ganti tema",
    "greeting.morning": "Selamat pagi.", "greeting.day": "Selamat siang.", "greeting.afternoon": "Selamat sore.", "greeting.evening": "Selamat malam.",
    "home.lede": "Notulen rapi pas rapat selesai. Tanpa bot, tanpa langganan, semuanya di laptopmu.",
    "setup.title": "Siapkan Lariksa sekali saja",
    "setup.body": "Model AI diunduh ke folder Lariksa dan berjalan offline setelahnya.",
    "setup.go": "Unduh semua ({size})", "setup.going": "Mengunduh…", "setup.failed": "Gagal",
    "record.start.title": "Mulai rapat", "record.start.sub": "Rekam suaramu dan suara peserta dari Meet, Zoom, atau Teams.",
    "record.live.title": "Merekam · {time}", "record.live.sub": "Mic: {mic} · Audio rapat: {system}",
    "record.button": "Rekam", "record.stop": "Selesai", "record.title.placeholder": "Judul rapat (opsional)",
    "record.notready": "Unduh model AI dulu",
    "record.hotkey.start": " untuk mulai, dari aplikasi mana pun", "record.hotkey.stop": " untuk selesai, dari aplikasi mana pun",
    "meter.me": "Saya", "meter.others": "Peserta",
    "consent": "Beri tahu peserta bahwa rapat ini direkam.",
    "consent.mic": "Mic tidak menangkap suara. Cek tombol mute di laptop atau di Zoom/Meet.",
    "consent.system": "Belum ada suara peserta. Pastikan audio rapat keluar dari speaker/headset default.",
    "recent": "RAPAT TERBARU", "import": "Impor rekaman atau transkrip",
    "empty.search": "Tidak ada rapat yang cocok.", "empty.none": "Halaman baru untuk rapat berikutnya. Tekan Rekam saat rapat dimulai.",
    "card.failed": "Proses gagal. Buka untuk mencoba lagi.", "card.pending": "Notulen sedang disiapkan…",
    "card.actions": "{n} action item",
    // dates
    "date.today": "Hari ini, {time}", "date.yesterday": "Kemarin, {time}",
    "months": "Jan,Feb,Mar,Apr,Mei,Jun,Jul,Agu,Sep,Okt,Nov,Des",
    "days": "MINGGU,SENIN,SELASA,RABU,KAMIS,JUMAT,SABTU",
    // statuses
    "status.recording": "Merekam", "status.queued": "Menunggu", "status.transcribing": "Transkripsi",
    "status.summarizing": "Menyusun notulen", "status.done": "Selesai", "status.failed": "Gagal",
    "stage.queued": "Menunggu giliran", "stage.transcribing": "Mentranskripsi", "stage.summarizing": "Menyusun notulen",
    "progress.waiting.other": "Menunggu rapat lain selesai diproses", "progress.recording": "Sedang merekam",
    // meeting
    "back": "Kembali", "copy.wa": "Salin untuk WhatsApp", "regen": "Notulen", "regen.title": "Tulis ulang notulen dari transkrip",
    "delete": "Hapus", "meeting.title.aria": "Judul rapat",
    "tab.notulen": "Notulen", "tab.transcript": "Transkrip", "find.placeholder": "Cari di rapat ini",
    "meta.import": "Impor", "meta.record": "Rekaman",
    "meta.transcribe": "transkripsi {n} dtk", "meta.summarize": "notulen {n} dtk",
    "failed.default": "Proses gagal.", "retry": "Coba lagi", "retry.summary": "Ulangi notulen saja",
    "notulen.unavailable": "Notulen belum tersedia.", "notulen.pending": "Notulen akan muncul di sini setelah rapat diproses.",
    "notulen.legacy": "Notulen ini dibuat dengan versi lama. Tekan Notulen (ikon putar) untuk membuat ulang dengan bab, waktu, dan PIC.",
    "notulen.hint": "Klik teks mana pun untuk mengedit, centang tugas yang selesai. Klik waktu untuk lompat ke momennya.",
    "action.done.aria": "Selesai", "jump.title": "Lompat ke momen ini",
    "transcript.pending": "Transkrip akan muncul di sini.", "play.aria": "Putar",
    "toast.saved.recording": "Rekaman disimpan. Notulen sedang disiapkan.",
    "toast.imported": "Rekaman diimpor. Notulen sedang disiapkan.",
    "toast.copied": "Disalin. Tempel di WhatsApp.", "toast.docx": "DOCX tersimpan.", "toast.md": "Markdown tersimpan.",
    "toast.deleted": "Rapat dihapus.",
    "confirm.regen": "Tulis ulang notulen dari transkrip? Suntingan pada notulen ini akan diganti.",
    "confirm.delete": "Hapus rapat ini beserta rekaman audionya? Tindakan ini tidak bisa dibatalkan.",
    // settings
    "settings.eyebrow": "PENGATURAN", "settings.title": "Pengaturan",
    "s.ui.label": "Bahasa tampilan", "s.ui.note": "Bahasa menu, tombol, dan pesan di aplikasi.",
    "s.lang.label": "Bahasa yang diucapkan di rapat", "s.lang.id": "Bahasa Indonesia (campur Inggris)", "s.lang.en": "English",
    "s.lang.auto": "Deteksi otomatis", "s.lang.note": "Membantu transkripsi. Kalimat bahasa Inggris tetap ditulis dalam bahasa Inggris.",
    "s.notulen.label": "Bahasa notulen", "s.notulen.auto": "Ikuti bahasa rapat", "s.notulen.id": "Selalu Bahasa Indonesia",
    "s.notulen.en": "Selalu Bahasa Inggris",
    "s.notulen.note": "\"Ikuti bahasa rapat\": rapat berbahasa Inggris menghasilkan notulen berbahasa Inggris.",
    "s.whisper.label": "Model transkripsi", "s.whisper.auto": "Otomatis (paling akurat)", "s.whisper.small": "Whisper small (lebih ringan)",
    "s.glossary.label": "Glosarium: nama orang, produk, istilah", "s.glossary.note": "Dipakai untuk membantu AI mengeja nama dan istilah dengan benar.",
    "s.hotkey.label": "Pintasan rekam / selesai (berfungsi di aplikasi mana pun)",
    "s.hotkey.note": "Tekan saat Zoom atau Meet terbuka untuk mulai atau selesai merekam. Kosongkan untuk mematikan.",
    "s.pill": "Tampilkan indikator mengambang saat merekam dan memproses",
    "s.save": "Simpan", "s.saved": "Semua perubahan tersimpan", "s.models": "MODEL & MESIN",
    "s.fine": "Semua audio, transkrip, dan notulen disimpan di folder data di samping aplikasi.",
    "asset.installed": "Terpasang", "asset.download": "Unduh", "asset.retry": "Coba lagi",
    // pill
    "pill.recording": "Merekam", "pill.recording.hotkey": "Merekam · {hotkey}", "pill.mic.silent": "Mic tidak terdengar",
    "pill.system.silent": "Belum ada suara peserta", "pill.processing": "Memproses", "pill.queue": " · {n} antre",
    "pill.stage.queued": "Menunggu", "pill.stage.transcribing": "Transkripsi", "pill.stage.summarizing": "Menyusun notulen",
    "pill.done": "Selesai", "pill.ready": "Notulen siap", "pill.stop": "Selesai", "pill.stop.title": "Selesai merekam",
    "pill.stop.title.hotkey": "Selesai merekam ({hotkey})", "pill.open": "Buka", "pill.close": "Tutup indikator",
  },
  en: {
    "nav.home": "Home", "nav.settings": "Settings", "nav.meetings": "MEETINGS",
    "search.placeholder": "Search your notes", "search.aria": "Meetings",
    "engine": "Engine: {label}",
    "theme": "Theme", "theme.aria": "Switch theme",
    "greeting.morning": "Good morning.", "greeting.day": "Good afternoon.", "greeting.afternoon": "Good afternoon.", "greeting.evening": "Good evening.",
    "home.lede": "Clean minutes the moment your meeting ends. No bot, no subscription, everything on your laptop.",
    "setup.title": "Set up Lariksa once",
    "setup.body": "The AI models download into the Lariksa folder and run offline after that.",
    "setup.go": "Download all ({size})", "setup.going": "Downloading…", "setup.failed": "Failed",
    "record.start.title": "Start a meeting", "record.start.sub": "Record your voice and everyone else's from Meet, Zoom or Teams.",
    "record.live.title": "Recording · {time}", "record.live.sub": "Mic: {mic} · Meeting audio: {system}",
    "record.button": "Record", "record.stop": "Stop", "record.title.placeholder": "Meeting title (optional)",
    "record.notready": "Download the AI models first",
    "record.hotkey.start": " to start, from any app", "record.hotkey.stop": " to stop, from any app",
    "meter.me": "Me", "meter.others": "Others",
    "consent": "Tell participants that this meeting is being recorded.",
    "consent.mic": "Your mic is not picking up sound. Check the mute button on your laptop or in Zoom/Meet.",
    "consent.system": "No sound from the other participants yet. Make sure meeting audio plays through your default speakers/headset.",
    "recent": "RECENT MEETINGS", "import": "Import a recording or transcript",
    "empty.search": "No meetings match.", "empty.none": "A blank page for your next meeting. Press Record when it starts.",
    "card.failed": "Processing failed. Open it to try again.", "card.pending": "Preparing the minutes…",
    "card.actions": "{n} action items",
    "date.today": "Today, {time}", "date.yesterday": "Yesterday, {time}",
    "months": "Jan,Feb,Mar,Apr,May,Jun,Jul,Aug,Sep,Oct,Nov,Dec",
    "days": "SUNDAY,MONDAY,TUESDAY,WEDNESDAY,THURSDAY,FRIDAY,SATURDAY",
    "status.recording": "Recording", "status.queued": "Waiting", "status.transcribing": "Transcribing",
    "status.summarizing": "Writing minutes", "status.done": "Done", "status.failed": "Failed",
    "stage.queued": "Waiting its turn", "stage.transcribing": "Transcribing", "stage.summarizing": "Writing the minutes",
    "progress.waiting.other": "Waiting for another meeting to finish", "progress.recording": "Recording now",
    "back": "Back", "copy.wa": "Copy for WhatsApp", "regen": "Minutes", "regen.title": "Rewrite the minutes from the transcript",
    "delete": "Delete", "meeting.title.aria": "Meeting title",
    "tab.notulen": "Minutes", "tab.transcript": "Transcript", "find.placeholder": "Search this meeting",
    "meta.import": "Imported", "meta.record": "Recorded",
    "meta.transcribe": "transcribed in {n} s", "meta.summarize": "minutes in {n} s",
    "failed.default": "Processing failed.", "retry": "Try again", "retry.summary": "Redo the minutes only",
    "notulen.unavailable": "No minutes yet.", "notulen.pending": "The minutes will appear here once the meeting is processed.",
    "notulen.legacy": "These minutes were made by an older version. Press Minutes (the redo button) to rebuild them with chapters, times and owners.",
    "notulen.hint": "Click any text to edit it, tick tasks that are done. Click a time to jump to that moment.",
    "action.done.aria": "Done", "jump.title": "Jump to this moment",
    "transcript.pending": "The transcript will appear here.", "play.aria": "Play",
    "toast.saved.recording": "Recording saved. Preparing the minutes.",
    "toast.imported": "Recording imported. Preparing the minutes.",
    "toast.copied": "Copied. Paste it into WhatsApp.", "toast.docx": "DOCX saved.", "toast.md": "Markdown saved.",
    "toast.deleted": "Meeting deleted.",
    "confirm.regen": "Rewrite the minutes from the transcript? Your edits to these minutes will be replaced.",
    "confirm.delete": "Delete this meeting and its audio recording? This cannot be undone.",
    "settings.eyebrow": "SETTINGS", "settings.title": "Settings",
    "s.ui.label": "Display language", "s.ui.note": "The language of menus, buttons and messages in the app.",
    "s.lang.label": "Language spoken in meetings", "s.lang.id": "Indonesian (mixed with English)", "s.lang.en": "English",
    "s.lang.auto": "Detect automatically", "s.lang.note": "Helps transcription. English sentences are still written in English.",
    "s.notulen.label": "Language of the minutes", "s.notulen.auto": "Follow the meeting", "s.notulen.id": "Always Indonesian",
    "s.notulen.en": "Always English",
    "s.notulen.note": "\"Follow the meeting\": a meeting held in English gets minutes in English.",
    "s.whisper.label": "Transcription model", "s.whisper.auto": "Automatic (most accurate)", "s.whisper.small": "Whisper small (lighter)",
    "s.glossary.label": "Glossary: people, products, terms", "s.glossary.note": "Helps the AI spell names and terms correctly.",
    "s.hotkey.label": "Record / stop shortcut (works in any app)",
    "s.hotkey.note": "Press it while Zoom or Meet is open to start or stop recording. Leave empty to turn it off.",
    "s.pill": "Show a floating indicator while recording and processing",
    "s.save": "Save", "s.saved": "All changes saved", "s.models": "MODELS & ENGINES",
    "s.fine": "All audio, transcripts and minutes are stored in the data folder next to the app.",
    "asset.installed": "Installed", "asset.download": "Download", "asset.retry": "Try again",
    "pill.recording": "Recording", "pill.recording.hotkey": "Recording · {hotkey}", "pill.mic.silent": "Mic is silent",
    "pill.system.silent": "No sound from others yet", "pill.processing": "Processing", "pill.queue": " · {n} queued",
    "pill.stage.queued": "Waiting", "pill.stage.transcribing": "Transcribing", "pill.stage.summarizing": "Writing minutes",
    "pill.done": "Done", "pill.ready": "Minutes ready", "pill.stop": "Stop", "pill.stop.title": "Stop recording",
    "pill.stop.title.hotkey": "Stop recording ({hotkey})", "pill.open": "Open", "pill.close": "Close indicator",
  },
};

let uiLang = "id";
let applied = false;

function t(key, vars = {}) {
  const text = (I18N[uiLang] && I18N[uiLang][key]) ?? I18N.id[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, k) => (vars[k] ?? `{${k}}`));
}

/** Set the language and redraw every marked element. Returns false when nothing changed. */
function setUiLanguage(lang) {
  const next = I18N[lang] ? lang : "id";
  if (next === uiLang && applied) return false;
  applied = true;
  uiLang = next;
  document.documentElement.lang = uiLang;
  document.querySelectorAll("[data-i18n]").forEach((n) => { n.textContent = t(n.dataset.i18n); });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((n) => { n.placeholder = t(n.dataset.i18nPlaceholder); });
  document.querySelectorAll("[data-i18n-title]").forEach((n) => { n.title = t(n.dataset.i18nTitle); });
  document.querySelectorAll("[data-i18n-aria]").forEach((n) => { n.setAttribute("aria-label", t(n.dataset.i18nAria)); });
  return true;
}
