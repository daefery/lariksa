//! The display language for messages the app shows (errors, asset names,
//! default meeting titles). It follows the "ui_language" setting, like the
//! screens in ui/i18n.js. Notulen and transcripts are not affected: they follow
//! the meeting's own language.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub const LANGUAGES: [&str; 2] = ["id", "en"];

pub fn set_language(lang: &str) {
    ENGLISH.store(lang == "en", Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// The Indonesian or English text, for the current display language.
pub fn tr(id: &'static str, en: &'static str) -> &'static str {
    pick(is_english(), id, en)
}

pub fn pick<'a>(english: bool, id: &'a str, en: &'a str) -> &'a str {
    if english { en } else { id }
}

/// Tests that read or change the display language take this lock, so tests
/// running in parallel never see each other's language.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    #[test]
    fn pick_follows_the_language() {
        assert_eq!(super::pick(false, "Rapat", "Meeting"), "Rapat");
        assert_eq!(super::pick(true, "Rapat", "Meeting"), "Meeting");
    }
}
