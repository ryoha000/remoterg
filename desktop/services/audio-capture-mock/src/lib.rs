use anyhow::{anyhow, Context, Result};
use core_types::{AudioCaptureCommandReceiver, AudioCaptureMessage, AudioFrame, AudioFrameSender};
use std::io::Cursor;
use tracing::{debug, error, info};
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};

const WAV_DATA: &[u8] = include_bytes!("assets/audio.wav");

const FRAME_DURATION_MS: u32 = 10;
const SAMPLES_PER_FRAME: usize = 480; // 48000Hz * 10ms / 1000
const SAMPLES_PER_FRAME_STEREO: usize = SAMPLES_PER_FRAME * 2; // 960
const TIMER_RESOLUTION_MS: u32 = 1;

/// 線形補間によるリサンプリング（任意Hz → 48kHz）
fn resample_linear(samples: &[f32], src_rate: u32, dst_rate: u32, channels: u16) -> Vec<f32> {
    if src_rate == dst_rate {
        return samples.to_vec();
    }

    let ratio = src_rate as f64 / dst_rate as f64;
    let src_frames = samples.len() / channels as usize;
    let dst_frames = (src_frames as f64 / ratio).ceil() as usize;

    let mut output = Vec::with_capacity(dst_frames * channels as usize);

    for dst_frame_idx in 0..dst_frames {
        let src_pos = dst_frame_idx as f64 * ratio;
        let src_frame_idx = src_pos.floor() as usize;
        let frac = src_pos - src_frame_idx as f64;

        if src_frame_idx + 1 >= src_frames {
            // 最後のフレームはコピー
            for ch in 0..channels as usize {
                let idx = src_frame_idx * channels as usize + ch;
                output.push(samples.get(idx).copied().unwrap_or(0.0));
            }
        } else {
            // 線形補間: sample0 + (sample1 - sample0) * frac
            for ch in 0..channels as usize {
                let idx0 = src_frame_idx * channels as usize + ch;
                let idx1 = (src_frame_idx + 1) * channels as usize + ch;

                let sample0 = samples[idx0];
                let sample1 = samples[idx1];
                let interpolated = sample0 + (sample1 - sample0) * frac as f32;

                output.push(interpolated);
            }
        }
    }

    info!(
        "Resampled {}Hz → 48000Hz ({} frames → {} frames)",
        src_rate, src_frames, dst_frames
    );

    output
}

/// チャネル数を変換（ステレオに統一）
fn convert_to_stereo(samples: &[f32], src_channels: u16) -> Vec<f32> {
    match src_channels {
        2 => samples.to_vec(), // すでにステレオ
        1 => {
            // モノラル → ステレオ（両チャンネルに同じ値をコピー）
            let mut output = Vec::with_capacity(samples.len() * 2);
            for &sample in samples {
                output.push(sample);
                output.push(sample);
            }
            info!(
                "Converted mono to stereo: {} → {} samples",
                samples.len(),
                output.len()
            );
            output
        }
        _ => {
            // 3ch以上 → ステレオ（最初の2チャンネルのみ使用）
            let frames = samples.len() / src_channels as usize;
            let mut output = Vec::with_capacity(frames * 2);

            for frame_idx in 0..frames {
                let base_idx = frame_idx * src_channels as usize;
                output.push(samples[base_idx]); // L
                output.push(samples[base_idx + 1]); // R
            }

            info!(
                "Converted {}ch to stereo: {} → {} samples (using first 2 channels)",
                src_channels,
                samples.len(),
                output.len()
            );
            output
        }
    }
}

/// WAVファイルを読み込んで10msフレームに分割
fn load_audio_samples() -> Result<Vec<Vec<f32>>> {
    let cursor = Cursor::new(WAV_DATA);
    let mut reader = hound::WavReader::new(cursor)
        .context("Failed to parse WAV file. Ensure src/assets/audio.wav is a valid WAV file.")?;

    let spec = reader.spec();

    // サンプルレートの上限チェック
    if spec.sample_rate > 192000 {
        return Err(anyhow!(
            "Sample rate too high: {}Hz (maximum: 192000Hz)",
            spec.sample_rate
        ));
    }

    // チャンネル数の検証
    if spec.channels == 0 || spec.channels > 8 {
        return Err(anyhow!(
            "Invalid channel count: {} (supported: 1-8)",
            spec.channels
        ));
    }

    info!(
        "Loading WAV file: {}Hz, {} channels, {} bits, {:?} format",
        spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
    );

    // サンプルをf32配列に変換
    let all_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read float samples")?,
        hound::SampleFormat::Int => {
            // i16 または i32 から f32 に変換
            match spec.bits_per_sample {
                16 => reader
                    .samples::<i16>()
                    .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read i16 samples")?,
                32 => reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / i32::MAX as f32))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read i32 samples")?,
                _ => {
                    return Err(anyhow!(
                        "Unsupported bit depth: {}. Supported: 16, 32",
                        spec.bits_per_sample
                    ))
                }
            }
        }
    };

    info!("Loaded {} raw samples from WAV file", all_samples.len());

    // リサンプリング（任意Hz → 48kHz）
    let resampled = if spec.sample_rate != 48000 {
        resample_linear(&all_samples, spec.sample_rate, 48000, spec.channels)
    } else {
        all_samples
    };

    // チャネル変換（任意ch → 2ch）
    let stereo_samples = if spec.channels != 2 {
        convert_to_stereo(&resampled, spec.channels)
    } else {
        resampled
    };

    info!("Processed {} stereo samples @ 48kHz", stereo_samples.len());

    // 10msフレーム（960サンプル）に分割
    let mut frames = Vec::new();

    for chunk in stereo_samples.chunks(SAMPLES_PER_FRAME_STEREO) {
        // 最後のチャンクが不完全な場合はゼロパディング
        let mut samples = chunk.to_vec();
        if samples.len() < SAMPLES_PER_FRAME_STEREO {
            info!(
                "Last frame padded with zeros: {} -> {} samples",
                chunk.len(),
                SAMPLES_PER_FRAME_STEREO
            );
            samples.resize(SAMPLES_PER_FRAME_STEREO, 0.0);
        }

        frames.push(samples);
    }

    info!("Split into {} frames of 10ms each", frames.len());

    Ok(frames)
}

/// モックオーディオキャプチャサービス
pub struct AudioCaptureService {
    frame_tx: AudioFrameSender,
    command_rx: AudioCaptureCommandReceiver,
}

impl AudioCaptureService {
    pub fn new(frame_tx: AudioFrameSender, command_rx: AudioCaptureCommandReceiver) -> Self {
        Self {
            frame_tx,
            command_rx,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("AudioCaptureService (mock) started");

        // タイマー解像度を1msに設定
        unsafe {
            let result = timeBeginPeriod(TIMER_RESOLUTION_MS);
            if result != 0 {
                tracing::warn!(
                    "Failed to set timer resolution to {}ms",
                    TIMER_RESOLUTION_MS
                );
            } else {
                tracing::info!("Timer resolution set to {}ms", TIMER_RESOLUTION_MS);
            }
        }

        // 起動時にWAVファイルをロードしてフレーム分割
        let frames =
            load_audio_samples().context("Failed to load audio samples from embedded WAV file")?;

        info!("Loaded {} audio frames from WAV file", frames.len());

        let mut is_capturing = false;
        let mut frame_index = 0usize;
        let mut current_timestamp_us = 0u64;

        // 10ms間隔のタイマー（ドリフト補正あり）
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_millis(FRAME_DURATION_MS as u64));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // コマンド受信
                msg = self.command_rx.recv() => {
                    match msg {
                        Some(AudioCaptureMessage::Start { hwnd }) => {
                            info!("Start audio capture (mock) for HWND: {}", hwnd);
                            is_capturing = true;
                            frame_index = 0;
                            current_timestamp_us = 0;
                        }
                        Some(AudioCaptureMessage::Stop) => {
                            info!("Stop audio capture (mock)");
                            is_capturing = false;
                        }
                        None => {
                            debug!("Audio capture command channel closed");
                            break;
                        }
                    }
                }
                // 10msごとにフレーム送信
                _ = interval.tick() => {
                    if is_capturing {
                        // ループバック: 最後まで行ったら最初に戻る
                        let samples = frames[frame_index % frames.len()].clone();

                        let frame = AudioFrame {
                            samples,
                            sample_rate: 48000,
                            channels: 2,
                            timestamp_us: current_timestamp_us,
                        };

                        if let Err(e) = self.frame_tx.send(frame).await {
                            error!("Failed to send audio frame: {}", e);
                            break;
                        }

                        frame_index += 1;
                        current_timestamp_us += (FRAME_DURATION_MS as u64) * 1000; // 10ms → 10000us

                        // 定期的にログ出力（5秒ごと = 500フレーム）
                        if frame_index % 500 == 0 {
                            info!(
                                "Audio capture (mock) running: frame_index={}, looped={} times",
                                frame_index,
                                frame_index / frames.len()
                            );
                        }
                    }
                }
            }
        }

        // タイマー解像度を元に戻す
        unsafe {
            timeEndPeriod(TIMER_RESOLUTION_MS);
        }

        info!("AudioCaptureService (mock) stopped");
        Ok(())
    }
}
