use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::Arc;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

pub fn default_input_device_name() -> String {
    let host = cpal::default_host();
    match host.default_input_device() {
        Some(device) => device.name().unwrap_or_else(|err| {
            tracing::warn!(%err, "failed to read default input device name");
            "Unknown microphone".to_string()
        }),
        None => "No input device".to_string(),
    }
}

#[derive(Debug)]
pub struct AudioChunk {
    pub wav_bytes: Vec<u8>,
    pub duration_ms: u64,
    pub rms_avg: f32,
    pub rms_peak: f32,
}

pub struct Recorder {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    source_sample_rate: u32,
    channels: u16,
    started: std::time::Instant,
}

// cpal marks streams as not Send on all platforms. JustSay creates and stops the
// recorder from the hotkey worker thread on Windows; this marker is used so the
// controller can live behind Arc/Mutex shared with the Win32 message pump.
unsafe impl Send for Recorder {}

impl Recorder {
    pub fn start<F>(on_rms: F) -> anyhow::Result<Self>
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No microphone input device found"))?;
        let config = device
            .default_input_config()
            .context("No default microphone input config")?;
        let source_sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let on_rms = Arc::new(on_rms);
        let stream_config: cpal::StreamConfig = config.clone().into();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32, _>(
                &device,
                &stream_config,
                samples.clone(),
                channels,
                on_rms.clone(),
                |s| s,
            )?,
            cpal::SampleFormat::I16 => build_stream::<i16, _>(
                &device,
                &stream_config,
                samples.clone(),
                channels,
                on_rms.clone(),
                |s| s as f32 / i16::MAX as f32,
            )?,
            cpal::SampleFormat::U16 => build_stream::<u16, _>(
                &device,
                &stream_config,
                samples.clone(),
                channels,
                on_rms.clone(),
                |s| (s as f32 - 32768.0) / 32768.0,
            )?,
            sample => anyhow::bail!("Unsupported sample format: {sample:?}"),
        };

        stream.play().context("Start microphone stream")?;
        Ok(Self {
            stream,
            samples,
            source_sample_rate,
            channels,
            started: std::time::Instant::now(),
        })
    }

    pub fn stop(self) -> anyhow::Result<AudioChunk> {
        drop(self.stream);
        let duration_ms = self.started.elapsed().as_millis() as u64;
        let samples = self.samples.lock().clone();
        if samples.is_empty() {
            anyhow::bail!("No microphone samples captured");
        }
        let mono = to_mono(&samples, self.channels);
        let resampled = resample_linear(&mono, self.source_sample_rate, TARGET_SAMPLE_RATE);
        let (rms_avg, rms_peak) = audio_stats(&resampled);
        let wav_bytes = encode_wav_i16(&resampled, TARGET_SAMPLE_RATE);
        Ok(AudioChunk {
            wav_bytes,
            duration_ms,
            rms_avg,
            rms_peak,
        })
    }
}

fn build_stream<T, C>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    channels: u16,
    on_rms: Arc<dyn Fn(f32) + Send + Sync>,
    convert: C,
) -> anyhow::Result<cpal::Stream>
where
    T: cpal::SizedSample + Copy + Send + 'static,
    C: Fn(T) -> f32 + Send + Sync + 'static,
{
    let channels = channels as usize;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut mono_sum = 0.0_f32;
            let mut mono_count = 0usize;
            let mut out = samples.lock();
            out.reserve(data.len() / channels.max(1));
            for frame in data.chunks(channels.max(1)) {
                let mut frame_sum = 0.0;
                for sample in frame {
                    frame_sum += convert(*sample);
                }
                let mono = frame_sum / frame.len().max(1) as f32;
                out.push(mono);
                mono_sum += mono * mono;
                mono_count += 1;
            }
            if mono_count > 0 {
                let rms = (mono_sum / mono_count as f32).sqrt();
                on_rms((rms * 4.0).clamp(0.0, 1.0));
            }
        },
        move |err| {
            tracing::error!(%err, "microphone stream error");
        },
        None,
    )?;
    Ok(stream)
}

fn to_mono(samples: &[f32], _channels: u16) -> Vec<f32> {
    samples.to_vec()
}

fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.len() < 2 {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        output.push(a + (b - a) * frac);
    }
    output
}

fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&pcm.to_le_bytes());
    }
    out
}

fn audio_stats(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum_sq = 0.0_f32;
    let mut peak = 0.0_f32;
    for sample in samples {
        let abs = sample.abs();
        peak = peak.max(abs);
        sum_sq += sample * sample;
    }
    ((sum_sq / samples.len() as f32).sqrt(), peak)
}
