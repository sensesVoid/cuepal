//! The recognizer thread: owns the Vosk model/recognizer, never touches the
//! UI, never blocks on a lock. Audio arrives as native-rate chunks over a
//! channel; matches leave over another channel.

use crate::audio::{downmix_to_mono, NativeChunk, Resampler16k};
use crate::speech::ffi::{FfiError, Model, Recognizer, VoskApi};
use crate::speech::grammar::{build_grammar_json, detect_match};
use arc_swap::ArcSwap;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Dot states for the UI (FR-9; "off" is M1's manual mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicState {
    Listening,
    NoSignal,
    Error(String),
}

/// What the engine tells the UI.
#[derive(Debug)]
pub enum UiEvent {
    Match(MatchInfo),
    Mic(MicState),
}

/// A recognized cue. Logs hold timings only — no audio, no transcript.
#[derive(Debug, Clone)]
pub struct MatchInfo {
    pub cue: String,
    pub latency_ms: u32,
    #[allow(dead_code)]
    pub at: Instant, // reserved: spike-notes latency check; M1 formalizes
}

/// Handle to the running recognizer thread. M0: marker only — the thread
/// runs for the process lifetime; M1 adds a shutdown signal.
pub struct EngineHandle {}

pub struct Engine;

impl Engine {
    /// Build the recognizer and spawn the processing thread. Any FFI failure
    /// returns Err before any thread starts (FR-9: never crash the app).
    pub fn spawn(
        cues: Arc<ArcSwap<Vec<String>>>,
        events_tx: mpsc::Sender<UiEvent>,
        pcm_rx: mpsc::Receiver<NativeChunk>,
    ) -> Result<EngineHandle, FfiError> {
        let api = unsafe { VoskApi::load(None) }?;
        api.set_loglevel(0);
        let api = Arc::new(api);
        let model = unsafe { Model::new(&api, &model_path()) }?;
        let recognizer = unsafe { Recognizer::new(model, 16000.0) }?;
        recognizer.set_grammar(build_grammar_json(&cues.load().to_vec()).as_str());

        std::thread::Builder::new()
            .name("cuepal-recognizer".to_string())
            .spawn(move || run_loop(recognizer, cues, events_tx, pcm_rx))
            .map_err(|e| FfiError::Symbol(format!("spawn recognizer thread: {e}")))?;
        Ok(EngineHandle {})
    }
}

/// Resolve the model folder relative to the executable so the portable zip
/// works regardless of the process's working directory (PRD packaging).
pub fn model_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("model").to_string_lossy().to_string();
        }
    }
    "model".to_string()
}

fn run_loop(
    recognizer: Recognizer,
    cues: Arc<ArcSwap<Vec<String>>>,
    events_tx: mpsc::Sender<UiEvent>,
    pcm_rx: mpsc::Receiver<NativeChunk>,
) {
    let mut resampler: Option<Resampler16k> = None;
    let mut last_grammar: Vec<String> = cues.load().to_vec();
    let mut last_matched: Option<String> = None;
    let mut sent_no_signal = false;

    loop {
        let chunk = match pcm_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => {
                if !sent_no_signal {
                    let _ = events_tx.send(UiEvent::Mic(MicState::NoSignal));
                    sent_no_signal = true;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        sent_no_signal = false;
        let _ = events_tx.send(UiEvent::Mic(MicState::Listening));

        // Cue changes rebuild the grammar off-thread (PRD architecture rule).
        let current_cues = cues.load().to_vec();
        if current_cues != last_grammar {
            recognizer.set_grammar(build_grammar_json(&current_cues).as_str());
            last_grammar = current_cues.clone();
            last_matched = None;
        }

        let res = match resampler.as_mut() {
            Some(r) => r,
            None => match Resampler16k::new(chunk.rate) {
                Ok(r) => resampler.insert(r),
                Err(e) => {
                    let _ = events_tx.send(UiEvent::Mic(MicState::Error(e.to_string())));
                    continue;
                }
            },
        };
        let mut chunks: Vec<Vec<i16>> = Vec::new();
        if let Err(e) = res.push(&downmix_to_mono(&chunk.samples, chunk.channels), &mut chunks) {
            let _ = events_tx.send(UiEvent::Mic(MicState::Error(e.to_string())));
            continue;
        }

        for piece in &chunks {
            let speech_ended = recognizer.accept_waveform(piece);
            let mut text = recognizer.partial_result();
            if speech_ended {
                let final_text = recognizer.final_result();
                if !final_text.is_empty() {
                    text = if text.is_empty() {
                        final_text
                    } else {
                        format!("{text} {final_text}")
                    };
                }
            }
            if text.is_empty() {
                continue;
            }
            if let Some(idx) = detect_match(&text, &current_cues) {
                let cue = current_cues[idx].clone();
                if should_report_match(&last_matched, &cue) {
                    let latency_ms = (Instant::now().saturating_duration_since(chunk.captured_at))
                        .as_millis() as u32;
                    log::info!("match cue_len={} latency_ms={latency_ms}", cue.len());
                    let _ = events_tx.send(UiEvent::Match(MatchInfo {
                        cue: cue.clone(),
                        latency_ms,
                        at: chunk.captured_at,
                    }));
                }
                last_matched = Some(cue);
                if speech_ended {
                    last_matched = None;
                }
            }
        }
    }
}

/// Pure decision (testable): report a match unless it is the exact same cue
/// that is still being spoken (intra-utterance repeat).
pub fn should_report_match(last: &Option<String>, cue: &str) -> bool {
    match last {
        None => true,
        Some(prev) => prev != cue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_first_match() {
        assert!(should_report_match(&None, "on system architecture"));
    }
    #[test]
    fn suppress_intra_utterance_repeat() {
        assert!(!should_report_match(&Some("on system architecture".to_string()), "on system architecture"));
    }
    #[test]
    fn report_different_cue() {
        assert!(should_report_match(&Some("on system architecture".to_string()), "why this company"));
    }
    #[test]
    fn model_path_ends_in_model() {
        assert!(model_path().ends_with("model"));
    }
    #[test]
    fn model_path_is_absolute_when_possible() {
        assert!(std::path::Path::new(&model_path()).is_absolute());
    }
}
