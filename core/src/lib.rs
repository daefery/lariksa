//! Lariksa core: recording, transcription, notulen, storage and exports.
//! Everything except the window, so it is testable on any OS.

pub mod audio;
pub mod catalog;
pub mod export;
pub mod hardware;
pub mod hotkey;
pub mod i18n;
pub mod language;
pub mod llm;
pub mod media;
pub mod minutes;
pub mod minutes_writer;
pub mod overlay;
pub mod paths;
pub mod pipeline;
pub mod process;
pub mod prompts;
pub mod recorder;
pub mod render;
pub mod service;
pub mod store;
pub mod text;
pub mod transcribe;
pub mod transcript;
pub mod transcript_import;
