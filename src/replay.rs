//! Replay harness for M1 gates (latency, recall, false triggers).
//!
//! Test data lives in `testdata/` (gitignored - `*.wav` ignored):
//! - `testdata/latency.wav` + `testdata/latency.expected.json` (cue index +
//!   expected utterance timings)
//! - `testdata/recall/` (10 cues x 5 reps)
//! - `testdata/unrelated-30min.wav`
//!
//! Run locally: `cargo test -- --ignored`

#![allow(dead_code)]

use crate::audio::NativeChunk;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Feed PCM samples to the engine as timed chunks, then drop the sender to
/// signal end-of-stream.
pub fn feed_pcm(
    pcm_tx: mpsc::Sender<NativeChunk>,
    samples: &[i16],
    rate: u32,
    channels: u16,
    chunk_ms: u32,
) {
    let frames_per_chunk = ((rate as u64 * chunk_ms as u64) / 1000).max(1) as usize;
    let samples_per_chunk = frames_per_chunk * channels as usize;
    let base = Instant::now();
    let mut offset = 0usize;
    let mut chunk_idx: u64 = 0;
    while offset < samples.len() {
        let end = (offset + samples_per_chunk).min(samples.len());
        let chunk = NativeChunk {
            samples: samples[offset..end]
                .iter()
                .map(|&s| s as f32 / i16::MAX as f32)
                .collect(),
            rate,
            channels,
            captured_at: base + Duration::from_millis(chunk_idx * chunk_ms as u64),
        };
        if pcm_tx.send(chunk).is_err() {
            break;
        }
        offset = end;
        chunk_idx += 1;
    }
    // drop pcm_tx by letting it go out of scope
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::M1_NOTES;
    use crate::speech::engine::{Engine, UiEvent};
    use arc_swap::ArcSwap;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    fn p95(mut values: Vec<u32>) -> u32 {
        if values.is_empty() {
            return 0;
        }
        values.sort_unstable();
        let idx = ((values.len() as f64) * 0.95).ceil() as usize;
        let idx = idx.saturating_sub(1).min(values.len() - 1);
        values[idx]
    }

    #[test]
    #[ignore = "requires local model + labeled audio"]
    fn latency_p95_under_400ms() {
        // Load WAV
        let mut reader = hound::WavReader::open("testdata/latency.wav")
            .expect("testdata/latency.wav required");
        let spec = reader.spec();
        let channels = spec.channels;
        let rate = spec.sample_rate;
        let samples: Vec<i16> = match spec.sample_format {
            hound::SampleFormat::Int => reader
                .samples::<i16>()
                .map(|s| s.expect("sample"))
                .collect(),
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .map(|s| (s.expect("sample") * i16::MAX as f32) as i16)
                .collect(),
        };

        // Optional expected timings (documented format)
        let _expected: Option<serde_json::Value> = std::fs::read_to_string("testdata/latency.expected.json")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());

        let cues = Arc::new(ArcSwap::from_pointee(M1_NOTES));
        let (events_tx, events_rx) = mpsc::channel();
        let (pcm_tx, pcm_rx) = mpsc::channel::<NativeChunk>();

        Engine::spawn(true, cues, events_tx, pcm_rx).expect("engine spawn");
        feed_pcm(pcm_tx, &samples, rate, channels, 20);

        let mut latencies: Vec<u32> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            match events_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(UiEvent::Match { latency_ms, .. }) => latencies.push(latency_ms),
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // keep waiting until deadline; engine may still be processing
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(!latencies.is_empty(), "no matches collected");
        let p95_ms = p95(latencies);
        log::info!("latency p95_ms={p95_ms}");
        assert!(p95_ms <= 400, "p95 latency {p95_ms}ms exceeds 400ms");
    }

    #[test]
    #[ignore = "requires local model + labeled audio"]
    fn recall_on_labeled_set() {
        // 10 cues x 5 reps; assert >= 95% open the right card.
        todo!()
    }

    #[test]
    #[ignore = "requires local model + 30-min unrelated speech"]
    fn false_triggers_under_one_per_30min() {
        // Replay 30 min of unrelated speech; assert <= 1 match.
        todo!()
    }
}
