//! The recognizer thread: owns the Vosk model/recognizer, never touches the
//! UI, never blocks on a lock. Audio arrives as native-rate chunks over a
//! channel; matches leave over another channel.

use crate::audio::{downmix_to_mono, NativeChunk, Resampler16k};
use crate::notes::{build_grammar_json_from_topics, CueIndex};
use crate::speech::ffi::{FfiError, Model, Recognizer, VoskApi};
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
    #[allow(dead_code)]
    Off,
}

/// What the engine tells the UI.
#[derive(Debug)]
pub enum UiEvent {
    Match {
        topic: usize,
        latency_ms: u32,
        #[allow(dead_code)]
        at: Instant,
    },
    Close,
    SpeechEnded {
        #[allow(dead_code)]
        at: Instant,
    },
    Mic(MicState),
}

/// A recognized cue. Logs hold timings only — no audio, no transcript.
#[derive(Debug, Clone)]
pub struct MatchInfo {
    pub topic: usize,
    pub latency_ms: u32,
    #[allow(dead_code)]
    pub at: Instant,
}

/// Handle to the running recognizer thread. M0: marker only.
pub struct EngineHandle {}

pub struct Engine;

impl Engine {
    /// Build the recognizer and spawn the processing thread. Any FFI failure
    /// returns Err before any thread starts (FR-9). When `mic_enabled` is false
    /// (manual mode), no FFI load or thread is started.
    pub fn spawn(
        mic_enabled: bool,
        cues: Arc<ArcSwap<CueIndex>>,
        events_tx: mpsc::Sender<UiEvent>,
        pcm_rx: mpsc::Receiver<NativeChunk>,
    ) -> Result<EngineHandle, FfiError> {
        if !mic_enabled {
            return Ok(EngineHandle {});
        }
        let api = unsafe { VoskApi::load(None) }?;
        api.set_loglevel(0);
        let api = Arc::new(api);
        let model = unsafe { Model::new(&api, &model_path()) }?;
        let recognizer = unsafe { Recognizer::new(model, 16000.0) }?;
        let topics: CueIndex = **cues.load();
        recognizer.set_grammar(build_grammar_json_from_topics(topics).as_str());

        std::thread::Builder::new()
            .name("cuepal-recognizer".to_string())
            .spawn(move || run_loop(recognizer, cues, events_tx, pcm_rx))
            .map_err(|e| FfiError::Symbol(format!("spawn recognizer thread: {e}")))?;
        Ok(EngineHandle {})
    }
}

/// Resolve the model folder relative to the executable.
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
    cues: Arc<ArcSwap<CueIndex>>,
    events_tx: mpsc::Sender<UiEvent>,
    pcm_rx: mpsc::Receiver<NativeChunk>,
) {
    let mut resampler: Option<Resampler16k> = None;
    let mut last_grammar: CueIndex = **cues.load();
    let mut last_matched: Option<usize> = None;
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

        let current_topics: CueIndex = **cues.load();
        if current_topics != last_grammar {
            recognizer.set_grammar(build_grammar_json_from_topics(current_topics).as_str());
            last_grammar = current_topics;
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
        let mut pieces: Vec<Vec<i16>> = Vec::new();
        if let Err(e) = res.push(&downmix_to_mono(&chunk.samples, chunk.channels), &mut pieces) {
            let _ = events_tx.send(UiEvent::Mic(MicState::Error(e.to_string())));
            continue;
        }

        for piece in &pieces {
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

            // Closing cue of the open card -> Close.
            if let Some(open) = last_matched {
                let closing: Vec<String> =
                    current_topics[open].closing_cues.iter().map(|s| s.to_string()).collect();
                if crate::matcher::detect_closing(&text, &closing) {
                    let _ = events_tx.send(UiEvent::Close);
                    last_matched = None;
                    continue;
                }
            }

            // Longest contained cue wins (matcher).
            if let Some(topic) = crate::matcher::detect_cue(&text, current_topics, last_matched) {
                if should_report_match(&last_matched, topic) {
                    let latency_ms = (Instant::now().saturating_duration_since(chunk.captured_at))
                        .as_millis() as u32;
                    log::info!("match topic={topic} latency_ms={latency_ms}");
                    let _ = events_tx.send(UiEvent::Match {
                        topic,
                        latency_ms,
                        at: chunk.captured_at,
                    });
                }
                last_matched = Some(topic);
                if speech_ended {
                    last_matched = None;
                }
            }
            if speech_ended {
                let _ = events_tx.send(UiEvent::SpeechEnded { at: chunk.captured_at });
            }
        }
    }
}

pub fn should_report_match(last: &Option<usize>, topic: usize) -> bool {
    match last {
        None => true,
        Some(l) => *l != topic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_report_first_match() {
        assert!(should_report_match(&None, 0));
    }
    #[test]
    fn suppress_same_topic_repeat() {
        assert!(!should_report_match(&Some(0), 0));
    }
    #[test]
    fn report_different_topic() {
        assert!(should_report_match(&Some(0), 1));
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
