//! Serve meeting audio to the UI so timestamps can jump to the exact moment.
//!
//! A tiny HTTP server on 127.0.0.1 with a random port and a random per-run token
//! in every URL; only that token's holder (the app's own WebView) can fetch audio.
//! It supports HTTP Range, which <audio> needs to seek inside long recordings.
//!
//! For recorded meetings the two tracks (mic + system) are mixed into one
//! playback file on first request and cached next to them; imported audio is
//! served as-is.

use regex::Regex;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use tiny_http::{Header, Request, Response, Server, StatusCode};

use crate::audio::mix_tracks;
use crate::pipeline::{imported_audio, MIC_FILE, SYSTEM_FILE};
use crate::text::token_urlsafe;

pub const PLAYBACK: &str = "playback.wav";

static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w-]{6,64}$").unwrap());
static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^bytes=(\d*)-(\d*)$").unwrap());

/// Fixed types: the OS registry differs per machine and can be edited by other apps.
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("ogg" | "opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("webm") => "audio/webm",
        Some("mp4") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

type FolderFor = Arc<dyn Fn(&str) -> PathBuf + Send + Sync>;

pub struct MediaServer {
    pub token: String,
    pub port: u16,
    server: Arc<Server>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MediaServer {
    /// ``folder_for(meeting_id)`` gives the meeting's recording folder.
    pub fn start(folder_for: FolderFor) -> std::io::Result<MediaServer> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(std::io::Error::other)?);
        let port = server.server_addr().to_ip().map_or(0, |a| a.port());
        let token = token_urlsafe(18);
        let mix_lock = Arc::new(Mutex::new(()));
        let thread = {
            let (server, token) = (server.clone(), token.clone());
            std::thread::Builder::new().name("lariksa-media".into()).spawn(move || {
                for request in server.incoming_requests() {
                    let (token, folder_for, mix_lock) = (token.clone(), folder_for.clone(), mix_lock.clone());
                    // One thread per request: the player opens several ranged connections.
                    std::thread::spawn(move || handle(request, &token, &*folder_for, &mix_lock));
                }
            })?
        };
        Ok(MediaServer { token, port, server, thread: Some(thread) })
    }

    pub fn url(&self, meeting_id: &str) -> String {
        format!("http://127.0.0.1:{}/{}/{}", self.port, self.token, meeting_id)
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn audio_file(folder: &Path, mix_lock: &Mutex<()>) -> Option<PathBuf> {
    if !folder.is_dir() {
        return None;
    }
    let tracks: Vec<PathBuf> = [MIC_FILE, SYSTEM_FILE]
        .iter()
        .map(|n| folder.join(n))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 1024))
        .collect();
    if !tracks.is_empty() {
        let cached = folder.join(PLAYBACK);
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let newest = tracks.iter().filter_map(|t| modified(t)).max();
        let _guard = mix_lock.lock().unwrap_or_else(|e| e.into_inner());
        if !cached.exists() || modified(&cached) < newest {
            if let Err(e) = mix_tracks(&tracks, &cached) {
                log::error!("Could not prepare audio in {}: {e:#}", folder.display());
                return None;
            }
        }
        return Some(cached);
    }
    imported_audio(folder)
}

fn equal_time(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn handle(req: Request, token: &str, folder_for: &(dyn Fn(&str) -> PathBuf + Send + Sync), mix_lock: &Mutex<()>) {
    let path = req.url().split('?').next().unwrap_or("").trim_matches('/').to_string();
    let parts: Vec<&str> = path.split('/').collect();
    let not_found = || Response::empty(StatusCode(404));
    if parts.len() != 2 || !equal_time(parts[0], token) || !ID.is_match(parts[1]) {
        let _ = req.respond(not_found());
        return;
    }
    let Some(file) = audio_file(&folder_for(parts[1]), mix_lock) else {
        let _ = req.respond(not_found());
        return;
    };
    let Ok(mut f) = File::open(&file) else {
        let _ = req.respond(not_found());
        return;
    };
    let size = f.metadata().map_or(0, |m| m.len());
    let range = req.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.as_str().trim().to_string());
    let (mut start, mut end) = (0u64, size.saturating_sub(1));
    let ranged = range.as_deref().and_then(|r| RANGE.captures(r)).map(|m| (m[1].to_string(), m[2].to_string()));
    if let Some((a, b)) = &ranged {
        if !a.is_empty() {
            start = a.parse().unwrap_or(0);
            end = if b.is_empty() { size.saturating_sub(1) } else { b.parse().unwrap_or(0) };
        } else if !b.is_empty() {
            start = size.saturating_sub(b.parse().unwrap_or(0)); // suffix range: last N bytes
        }
        if start > end || start >= size {
            let _ = req.respond(Response::empty(StatusCode(416)).with_header(header("Content-Range", &format!("bytes */{size}"))));
            return;
        }
        end = end.min(size - 1);
    }
    let length = end - start + 1;
    if f.seek(SeekFrom::Start(start)).is_err() {
        let _ = req.respond(not_found());
        return;
    }
    let mut response = Response::new(
        StatusCode(if ranged.is_some() { 206 } else { 200 }),
        vec![header("Content-Type", content_type(&file)), header("Accept-Ranges", "bytes"), header("Cache-Control", "no-store")],
        f.take(length),
        Some(length as usize),
        None,
    );
    if ranged.is_some() {
        response.add_header(header("Content-Range", &format!("bytes {start}-{end}/{size}")));
    }
    let _ = req.respond(response); // the player may hang up mid-stream when it seeks elsewhere
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::write_wav;

    fn get(url: &str, range: Option<&str>) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let mut req = ureq::get(url);
        if let Some(r) = range {
            req = req.set("Range", r);
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        let status = resp.status();
        let headers = resp.headers_names().into_iter().map(|h| (h.clone(), resp.header(&h).unwrap_or("").to_string())).collect();
        let mut body = vec![];
        resp.into_reader().read_to_end(&mut body).unwrap();
        (status, headers, body)
    }

    fn header_of<'a>(h: &'a [(String, String)], k: &str) -> &'a str {
        h.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map_or("", |(_, v)| v.as_str())
    }

    fn server(root: PathBuf) -> MediaServer {
        MediaServer::start(Arc::new(move |id: &str| root.join(id))).unwrap()
    }

    #[test]
    fn serves_mixed_tracks_with_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("20261009-083051-bfb80e");
        std::fs::create_dir_all(&folder).unwrap();
        write_wav(&folder.join(MIC_FILE), 16000, &[0; 16000]).unwrap();
        write_wav(&folder.join(SYSTEM_FILE), 16000, &[1; 16000]).unwrap();
        let srv = server(dir.path().to_path_buf());
        let url = srv.url("20261009-083051-bfb80e");

        let (status, headers, full) = get(&url, None);
        assert_eq!(status, 200);
        assert_eq!(header_of(&headers, "content-type"), "audio/wav");
        assert!(folder.join(PLAYBACK).exists());

        let (status, headers, part) = get(&url, Some("bytes=44-143"));
        assert_eq!(status, 206);
        assert_eq!(header_of(&headers, "content-range"), format!("bytes 44-143/{}", full.len()));
        assert_eq!(part, full[44..144]);

        assert_eq!(get(&url, Some("bytes=-10")).2, full[full.len() - 10..]);
        assert_eq!(get(&url, Some(&format!("bytes={}-", full.len() + 5))).0, 416);
    }

    #[test]
    fn wrong_token_or_path_is_404() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("abcdef123")).unwrap();
        write_wav(&dir.path().join("abcdef123").join(MIC_FILE), 16000, &[0; 4000]).unwrap();
        let srv = server(dir.path().to_path_buf());
        let base = format!("http://127.0.0.1:{}", srv.port);
        assert_eq!(get(&format!("{base}/wrong-token/abcdef123"), None).0, 404);
        assert_eq!(get(&format!("{base}/{}/..%2F..%2Fetc", srv.token), None).0, 404);
        assert_eq!(get(&format!("{base}/{}/missing-meeting", srv.token), None).0, 404);
        assert_eq!(get(&srv.url("abcdef123"), None).0, 200);
    }

    #[test]
    fn imported_audio_served_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("import-meeting");
        std::fs::create_dir_all(&folder).unwrap();
        let mut bytes = b"ID3".to_vec();
        bytes.extend([0u8; 2000]);
        std::fs::write(folder.join("import.mp3"), &bytes).unwrap();
        let srv = server(dir.path().to_path_buf());
        let (status, headers, body) = get(&srv.url("import-meeting"), None);
        assert_eq!(status, 200);
        assert_eq!(header_of(&headers, "content-type"), "audio/mpeg");
        assert!(body.starts_with(b"ID3"));
    }
}
