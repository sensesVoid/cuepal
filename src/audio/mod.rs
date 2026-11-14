//! Microphone capture. The cpal callback only copies audio into a channel —
//! no locks, no resampling on the audio thread (PRD architecture rule).

use std::sync::mpsc;
use std::time::Instant;

/// One native-rate capture burst from the device callback.
pub struct NativeChunk {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub rate: u32,
    pub captured_at: Instant,
}

#[derive(Debug)]
pub enum AudioStatus {
    #[allow(dead_code)]
    Running {
        #[allow(dead_code)]
        device: String,
        #[allow(dead_code)]
        rate: u32,
        #[allow(dead_code)]
        channels: u16,
    },
    Error(String),
}

#[derive(Debug)]
pub enum AudioError {
    NoDevice,
    Stream(String),
    Resample(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::NoDevice => write!(f, "no input device found"),
            AudioError::Stream(e) => write!(f, "audio stream error: {e}"),
            AudioError::Resample(e) => write!(f, "resampler error: {e}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// Owning handle: dropping it stops the capture stream.
pub struct AudioSession {
    #[allow(dead_code)] // kept for its Drop
    stream: cpal::Stream,
}

/// Open the default input device at its NATIVE rate/channels (many devices
/// refuse 16 kHz in shared mode — FR-9) and start streaming into `pcm_tx`.
pub fn start_capture(
    status_tx: mpsc::Sender<AudioStatus>,
    pcm_tx: mpsc::Sender<NativeChunk>,
) -> Result<AudioSession, AudioError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(AudioError::NoDevice)?;
    let name = device.to_string();
    let config = device
        .default_input_config()
        .map_err(|e| AudioError::Stream(e.to_string()))?;
    let (rate, channels) = (config.sample_rate(), config.channels());
    let _ = status_tx.send(AudioStatus::Running {
        device: name.clone(),
        rate,
        channels,
    });

    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    let err_tx = status_tx.clone();
    let stream = device
        .build_input_stream(
            stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Callback does nothing but copy — PRD architecture rule.
                let _ = pcm_tx.send(NativeChunk {
                    samples: data.to_vec(),
                    channels,
                    rate,
                    captured_at: Instant::now(),
                });
            },
            move |err| {
                log::error!("audio stream error: {err}");
                let _ = err_tx.send(AudioStatus::Error(err.to_string()));
            },
            None,
        )
        .map_err(|e| AudioError::Stream(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AudioError::Stream(e.to_string()))?;
    Ok(AudioSession { stream })
}

/// Average every group of `channels` interleaved samples into one frame.
pub fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return samples.to_vec();
    }
    samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Accumulates i16 samples, emits fixed-size chunks, holds the remainder.
pub struct I16Chunker {
    chunk_size: usize,
    leftover: Vec<i16>,
}

impl I16Chunker {
    pub fn new(chunk_size: usize) -> Self {
        Self {
            chunk_size,
            leftover: Vec::new(),
        }
    }

    pub fn push(&mut self, new: &[i16], out: &mut Vec<Vec<i16>>) {
        self.leftover.extend_from_slice(new);
        while self.leftover.len() >= self.chunk_size {
            out.push(self.leftover[..self.chunk_size].to_vec());
            self.leftover.drain(..self.chunk_size);
        }
    }
}

/// Realtime resampler from the device's native rate to 16 kHz mono.
/// Polynomial interpolation: the docs-recommended choice for a voice app on
/// a CPU budget; fixed-input mode suits cpal's callback-sized chunks.
pub struct Resampler16k {
    resampler: rubato::Async<f32>,
    pending: Vec<f32>,
    chunker: I16Chunker,
}

impl Resampler16k {
    pub fn new(native_rate: u32) -> Result<Self, AudioError> {
        let resampler = rubato::Async::<f32>::new_poly(
            (16000.0 / native_rate as f32) as f64,
            1.05,
            rubato::PolynomialDegree::Cubic,
            1024,
            1,
            rubato::FixedAsync::Input,
        )
        .map_err(|e| AudioError::Resample(e.to_string()))?;
        Ok(Self {
            resampler,
            pending: Vec::new(),
            chunker: I16Chunker::new(1600), // 100 ms at 16 kHz
        })
    }

    /// Feed mono samples at the native rate; append 16 kHz i16 100 ms chunks.
    pub fn push(&mut self, mono: &[f32], out: &mut Vec<Vec<i16>>) -> Result<(), AudioError> {
        use rubato::audioadapter_buffers::direct::InterleavedSlice;
        use rubato::{Indexing, Resampler};

        self.pending.extend_from_slice(mono);
        let indexing = Indexing::new();
        while self.pending.len() >= self.resampler.input_frames_next() {
            let frames = self.resampler.input_frames_next();
            let input = InterleavedSlice::new(&self.pending, 1, frames)
                .map_err(|e| AudioError::Resample(e.to_string()))?;
            let mut outdata = vec![0.0f32; self.resampler.output_frames_max()];
            let capacity = outdata.len();
            let mut output = InterleavedSlice::new_mut(&mut outdata, 1, capacity)
                .map_err(|e| AudioError::Resample(e.to_string()))?;
            let (_read, written) = self
                .resampler
                .process_into_buffer(&input, &mut output, Some(&indexing))
                .map_err(|e| AudioError::Resample(e.to_string()))?;
            self.pending.drain(..frames);
            let i16s: Vec<i16> = outdata[..written]
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            self.chunker.push(&i16s, out);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        let stereo = vec![0.4, -0.2, 0.6, 0.2]; // 2 frames, 2 channels
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.1, 0.4]);
    }

    #[test]
    fn chunker_cuts_on_100ms_boundaries() {
        let mut c = I16Chunker::new(1600); // 100 ms at 16 kHz
        let mut out = Vec::new();
        c.push(&vec![0i16; 3200], &mut out);
        assert_eq!(out.len(), 2);
        c.push(&vec![1i16; 2000], &mut out);
        assert_eq!(out.len(), 3); // one full chunk + 400 held
    }

    #[test]
    fn resampler_constructs_for_common_rates() {
        assert!(Resampler16k::new(48000).is_ok());
        assert!(Resampler16k::new(44100).is_ok());
    }
}
