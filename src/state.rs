#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Home,
    Active { id: usize },
}

pub struct StateCtx {
    manual_mode: bool,
    state: AppState,
    pinned: bool,
    shown_at_ms: u64,
}

impl StateCtx {
    pub fn new(manual_mode: bool) -> Self {
        Self {
            manual_mode,
            state: AppState::Home,
            pinned: false,
            shown_at_ms: 0,
        }
    }

    pub fn on_match(&mut self, id: usize, now_ms: u64) {
        self.state = AppState::Active { id };
        self.pinned = false;
        self.shown_at_ms = now_ms;
    }

    pub fn go_home(&mut self) {
        self.state = AppState::Home;
        self.pinned = false;
    }

    pub fn pin(&mut self) {
        if let AppState::Active { .. } = self.state {
            self.pinned = true;
        }
    }

    pub fn unpin(&mut self) {
        self.pinned = false;
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn manual_mode(&self) -> bool {
        self.manual_mode
    }

    pub fn state(&self) -> AppState {
        self.state
    }

    pub fn shown_at_ms(&self) -> u64 {
        self.shown_at_ms
    }
}

/// Pure reset decision (FR-7).
#[allow(clippy::too_many_arguments)]
pub fn should_reset(
    state: AppState,
    pinned: bool,
    manual: bool,
    speech_ended: bool,
    silence_ms: u64,
    dwell_ms: u64,
    reset_window_ms: u64,
    min_dwell_ms: u64,
) -> bool {
    match state {
        AppState::Home => false,
        AppState::Active { .. } => {
            if pinned || manual {
                return false;
            }
            if !speech_ended {
                return false;
            }
            if silence_ms < reset_window_ms {
                return false;
            }
            if dwell_ms < min_dwell_ms {
                return false;
            }
            true
        }
    }
}

/// Countdown before auto-reset; None when hidden (Home/pinned/manual).
pub fn countdown_ms(
    state: AppState,
    pinned: bool,
    manual: bool,
    silence_ms: u64,
    reset_window_ms: u64,
) -> Option<u64> {
    match state {
        AppState::Home => None,
        AppState::Active { .. } => {
            if pinned || manual {
                return None;
            }
            Some(reset_window_ms.saturating_sub(silence_ms))
        }
    }
}

/// Manual mode runs no capture stream.
pub fn should_open_capture_stream(manual_mode: bool) -> bool {
    !manual_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_opens_card() {
        let mut s = StateCtx::new(false);
        s.on_match(2, 0);
        assert_eq!(s.state(), AppState::Active { id: 2 });
    }

    #[test]
    fn closing_cue_returns_home() {
        let mut s = StateCtx::new(false);
        s.on_match(1, 0);
        s.go_home();
        assert_eq!(s.state(), AppState::Home);
    }

    #[test]
    fn auto_reset_requires_speech_ended_silence_and_dwell() {
        let mut s = StateCtx::new(false);
        s.on_match(0, 1000);
        // not enough dwell yet
        assert!(!should_reset(
            s.state(),
            s.is_pinned(),
            s.manual_mode(),
            true,
            3000,
            2000,
            2500,
            3000
        ));
        // dwell ok but silence too short
        assert!(!should_reset(
            s.state(),
            s.is_pinned(),
            s.manual_mode(),
            true,
            1000,
            4000,
            2500,
            3000
        ));
        // all conditions met
        assert!(should_reset(
            s.state(),
            s.is_pinned(),
            s.manual_mode(),
            true,
            3000,
            4000,
            2500,
            3000
        ));
    }

    #[test]
    fn pinned_never_auto_resets() {
        let mut s = StateCtx::new(false);
        s.on_match(0, 0);
        s.pin();
        assert!(!should_reset(
            s.state(),
            s.is_pinned(),
            s.manual_mode(),
            true,
            5000,
            10000,
            2500,
            3000
        ));
    }

    #[test]
    fn manual_mode_never_auto_resets() {
        let mut s = StateCtx::new(true);
        s.on_match(0, 0);
        assert!(!should_reset(
            s.state(),
            s.is_pinned(),
            s.manual_mode(),
            true,
            5000,
            10000,
            2500,
            3000
        ));
    }

    #[test]
    fn hotkey_home_always_returns_home() {
        let mut s = StateCtx::new(false);
        s.on_match(0, 0);
        s.pin();
        s.go_home();
        assert_eq!(s.state(), AppState::Home);
        assert!(!s.is_pinned());
    }

    #[test]
    fn countdown_hidden_when_pinned_or_manual() {
        assert_eq!(
            countdown_ms(AppState::Active { id: 0 }, true, false, 1000, 2500),
            None
        );
        assert_eq!(
            countdown_ms(AppState::Active { id: 0 }, false, true, 1000, 2500),
            None
        );
        assert_eq!(
            countdown_ms(AppState::Active { id: 0 }, false, false, 1000, 2500),
            Some(1500)
        );
    }

    #[test]
    fn should_open_capture_stream_gate() {
        assert!(!should_open_capture_stream(true));
        assert!(should_open_capture_stream(false));
    }
}
