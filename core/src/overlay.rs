//! The always-on-top recording pill, the global shortcut and taskbar progress.
//!
//! One small watcher follows the service state and keeps the three in sync, so
//! it does not matter how a recording started or stopped (main window button,
//! shortcut, pill button):
//!
//! ```text
//! recording            -> pill shows timer + levels + Selesai; taskbar red bar
//! processing           -> pill shows stage + %; taskbar green progress
//! done                 -> pill says "Notulen siap" for a few seconds, then hides
//! pill closed by user  -> stays hidden until the next recording
//! ```
//!
//! This module decides; the window layer only applies the decisions.

use serde_json::Value;

use crate::service::LariksaService;

pub const PILL_SIZE: (f64, f64) = (360.0, 58.0);
pub const MARGIN: f64 = 16.0;
pub const DONE_LINGER_S: f64 = 6.0;

/// Top centre of the screen, just below the edge (meeting apps put controls at the bottom).
pub fn pill_position(screen_x: f64, screen_y: f64, screen_w: f64) -> (f64, f64) {
    (screen_x + ((screen_w - PILL_SIZE.0) / 2.0).floor(), screen_y + MARGIN)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Taskbar {
    None,
    Indeterminate,
    /// 0..1
    Normal(f64),
    /// Recording: a full red bar.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording,
    Processing,
    Done,
}

/// What the watcher sees each tick.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub recording: bool,
    /// A meeting is being processed or waits in the queue.
    pub busy: bool,
    pub stage: String,
    pub progress: f64,
    pub show_pill: bool,
}

impl Inputs {
    pub fn of(svc: &LariksaService) -> Inputs {
        let activity = svc.activity();
        Inputs {
            recording: svc.recorder.recording(),
            busy: !activity.meeting_id.is_empty() || svc.queue_len() > 0,
            stage: activity.stage,
            progress: activity.progress,
            show_pill: svc.store.settings().get("show_pill").is_none_or(|v| !matches!(v, Value::Bool(false))),
        }
    }
}

#[derive(Debug, Default)]
pub struct Overlay {
    /// User closed the pill after a recording.
    dismissed: bool,
    pub visible: bool,
    done_at: Option<f64>,
    /// Recording or processing seen since the last "done".
    active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub state: State,
    pub pill_visible: bool,
    pub taskbar: Taskbar,
}

impl Overlay {
    pub fn hide_pill(&mut self) {
        self.dismissed = true;
        self.visible = false;
    }

    /// Advance the indicator state. ``now`` is a monotonic clock in seconds.
    pub fn tick(&mut self, i: &Inputs, was_recording: bool, now: f64) -> Decision {
        if i.recording && !was_recording {
            self.dismissed = false; // a new recording always brings the pill back
        }
        let (state, taskbar) = if i.recording {
            self.active = true;
            (State::Recording, Taskbar::Error)
        } else if i.busy {
            self.active = true;
            let progress = if i.stage != "queued" { i.progress } else { 0.0 };
            (State::Processing, if progress > 0.0 { Taskbar::Normal(progress) } else { Taskbar::Indeterminate })
        } else if self.active {
            // Work just finished: say so briefly, then get out of the way.
            self.active = false;
            self.done_at = Some(now);
            (State::Done, Taskbar::None)
        } else if self.done_at.is_some_and(|t| now - t < DONE_LINGER_S) {
            (State::Done, Taskbar::None)
        } else {
            self.done_at = None;
            (State::Idle, Taskbar::None)
        };
        self.visible = i.show_pill && !self.dismissed && state != State::Idle;
        Decision { state, pill_visible: self.visible, taskbar }
    }
}

/// What the global shortcut did.
#[derive(Debug, PartialEq, Eq)]
pub enum Toggle {
    Started,
    Stopped,
    /// Models missing or recording failed: the main window explains what to do.
    ShowMain,
}

pub fn toggle_recording(svc: &LariksaService) -> Toggle {
    log::info!("Shortcut pressed (recording={})", svc.recorder.recording());
    if svc.recorder.recording() {
        svc.stop_recording();
        return Toggle::Stopped;
    }
    if svc.status()["ready"] != true {
        return Toggle::ShowMain;
    }
    let result = svc.start_recording("");
    if result["ok"] == true {
        Toggle::Started
    } else {
        log::warn!("Hotkey start failed: {}", result["error"]);
        Toggle::ShowMain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(recording: bool) -> Inputs {
        Inputs { recording, show_pill: true, ..Default::default() }
    }

    #[test]
    fn full_lifecycle_record_process_done_hide() {
        let mut ov = Overlay::default();
        let mut t = 1000.0;
        assert_eq!(ov.tick(&rec(false), false, t).state, State::Idle);
        assert!(!ov.visible);

        let d = ov.tick(&rec(true), false, t);
        assert_eq!((d.state, d.pill_visible, d.taskbar), (State::Recording, true, Taskbar::Error));

        // Stopped: the meeting is queued, so the pill stays up while processing.
        let queued = Inputs { busy: true, stage: "queued".into(), show_pill: true, ..Default::default() };
        let d = ov.tick(&queued, true, t);
        assert_eq!((d.state, d.pill_visible, d.taskbar), (State::Processing, true, Taskbar::Indeterminate));

        let working = Inputs { busy: true, stage: "summarizing".into(), progress: 0.4, show_pill: true, ..Default::default() };
        assert_eq!(ov.tick(&working, false, t).taskbar, Taskbar::Normal(0.4));

        let d = ov.tick(&rec(false), false, t);
        assert_eq!((d.state, d.pill_visible, d.taskbar), (State::Done, true, Taskbar::None)); // "Notulen siap" lingers
        t += DONE_LINGER_S + 1.0;
        let d = ov.tick(&rec(false), false, t);
        assert_eq!((d.state, d.pill_visible), (State::Idle, false));
    }

    #[test]
    fn closing_the_pill_keeps_it_closed_until_the_next_recording() {
        let mut ov = Overlay::default();
        ov.tick(&rec(true), false, 0.0);
        ov.hide_pill();
        assert!(!ov.visible);
        let busy = Inputs { busy: true, show_pill: true, ..Default::default() };
        ov.tick(&busy, true, 0.0);
        assert!(!ov.visible); // processing, but the user closed it
        ov.tick(&rec(false), false, 0.0);
        ov.tick(&rec(true), false, 0.0);
        assert!(ov.visible); // new recording brings it back
    }

    #[test]
    fn setting_can_disable_the_pill() {
        let mut ov = Overlay::default();
        let d = ov.tick(&Inputs { recording: true, show_pill: false, ..Default::default() }, false, 0.0);
        assert_eq!((d.state, d.pill_visible), (State::Recording, false));
    }

    #[test]
    fn pill_sits_top_centre() {
        assert_eq!(pill_position(0.0, 0.0, 1920.0), (((1920 - 360) / 2) as f64, 16.0));
        assert_eq!(pill_position(1920.0, 0.0, 2560.0), ((1920 + (2560 - 360) / 2) as f64, 16.0)); // second monitor
    }
}
