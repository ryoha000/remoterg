use anyhow::{anyhow, Context, Result};
use core_types::{AudioCaptureCommandReceiver, AudioCaptureMessage, AudioFrame, AudioFrameSender};
use std::io::Cursor;
use tracing::{debug, error, info};

const WAV_DATA: &[u8] = include_bytes!("assets/audio.wav");

const FRAME_DURATION_MS: u32 = 10;
const SAMPLES_PER_FRAME: usize = 480; // 48000Hz * 10ms / 1000
const SAMPLES_PER_FRAME_STEREO: usize = SAMPLES_PER_FRAME * 2; // 960

/// WAVファイルを読み込んで10msフレームに分割
fn load_audio_samples() -> Result<Vec<Vec<f32>>> {
    let cursor = Cursor::new(WAV_DATA);
    let mut reader = hound::WavReader::new(cursor)
        .context("Failed to parse WAV file. Ensure src/assets/audio.wav is a valid WAV file.")?;

    let spec = reader.spec();

    // フォーマット検証
    if spec.sample_rate != 48000 {
        return Err(anyhow!(
            "WAV file must be 48000Hz (found: {}Hz). Please convert your audio file.",
            spec.sample_rate
        ));
    }
    if spec.channels != 2 {
        return Err(anyhow!(
            "WAV file must be stereo/2ch (found: {}ch). Please convert your audio file.",
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

    info!("Loaded {} samples from WAV file", all_samples.len());

    // 10msフレーム（960サンプル）に分割
    let mut frames = Vec::new();

    for chunk in all_samples.chunks(SAMPLES_PER_FRAME_STEREO) {
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

        // 起動時にWAVファイルをロードしてフレーム分割
        let frames = load_audio_samples()
            .context("Failed to load audio samples from embedded WAV file")?;

        info!("Loaded {} audio frames from WAV file", frames.len());

        let mut is_capturing = false;
        let mut frame_index = 0usize;
        let mut current_timestamp_us = 0u64;

        // 10ms間隔のタイマー（ドリフト補正あり）
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(
            FRAME_DURATION_MS as u64,
        ));
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

        info!("AudioCaptureService (mock) stopped");
        Ok(())
    }
}
