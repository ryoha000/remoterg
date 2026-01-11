use anyhow::Result;
use audio_encoder::{OpusEncoderFactory, OpusEncoderWrapper};
use core_types::{AudioEncoderFactory, AudioFrame};
use std::path::PathBuf;
use std::sync::Once;
use std::time::Duration;

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u16 = 2;
const FRAME_DURATION_MS: u32 = 10;
const SAMPLES_PER_FRAME: usize = 480; // 48000 * 10ms / 1000

struct SineWaveConfig {
    frequency: f32,
    amplitude: f32,
    duration_secs: f32,
}

static INIT_TRACING: Once = Once::new();

fn init_tracing() {
    INIT_TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .init();
    });
}

fn generate_sine_wave(config: SineWaveConfig) -> Vec<AudioFrame> {
    let total_samples = (SAMPLE_RATE as f32 * config.duration_secs) as usize;
    let num_frames = total_samples / SAMPLES_PER_FRAME;

    let mut frames = Vec::new();
    let mut timestamp_us = 0u64;

    for frame_idx in 0..num_frames {
        let mut samples = Vec::with_capacity(SAMPLES_PER_FRAME * CHANNELS as usize);

        for i in 0..SAMPLES_PER_FRAME {
            let t = (frame_idx * SAMPLES_PER_FRAME + i) as f32 / SAMPLE_RATE as f32;
            let value =
                config.amplitude * (2.0 * std::f32::consts::PI * config.frequency * t).sin();

            // ステレオ: L, R同じ値
            samples.push(value);
            samples.push(value);
        }

        frames.push(AudioFrame {
            samples,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            timestamp_us,
        });

        timestamp_us += (FRAME_DURATION_MS as u64) * 1000;
    }

    frames
}

struct OpusDecoderWrapper {
    decoder: *mut opus_sys::OpusDecoder,
}

impl OpusDecoderWrapper {
    fn new(sample_rate: i32, channels: i32) -> Result<Self> {
        let mut error: i32 = 0;
        let decoder =
            unsafe { opus_sys::opus_decoder_create(sample_rate, channels, &mut error as *mut i32) };

        if error != opus_sys::OPUS_OK as i32 || decoder.is_null() {
            return Err(anyhow::anyhow!(
                "Failed to create Opus decoder: error {}",
                error
            ));
        }

        Ok(Self { decoder })
    }

    fn decode_float(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize> {
        let frame_size = (output.len() / 2) as i32;
        let decoded_samples = unsafe {
            opus_sys::opus_decode_float(
                self.decoder,
                data.as_ptr(),
                data.len() as i32,
                output.as_mut_ptr(),
                frame_size,
                0,
            )
        };

        if decoded_samples < 0 {
            return Err(anyhow::anyhow!(
                "Decoding failed: error {}",
                decoded_samples
            ));
        }

        Ok((decoded_samples as usize) * 2)
    }
}

impl Drop for OpusDecoderWrapper {
    fn drop(&mut self) {
        unsafe {
            opus_sys::opus_decoder_destroy(self.decoder);
        }
    }
}

unsafe impl Send for OpusDecoderWrapper {}

fn calculate_rms(frames: &[AudioFrame]) -> f32 {
    let mut sum_squares: f64 = 0.0;
    let mut count: usize = 0;

    for frame in frames {
        for sample in &frame.samples {
            sum_squares += (*sample as f64) * (*sample as f64);
            count += 1;
        }
    }

    (sum_squares / count as f64).sqrt() as f32
}

fn calculate_peak(frames: &[AudioFrame]) -> f32 {
    frames
        .iter()
        .flat_map(|f| &f.samples)
        .map(|s| s.abs())
        .fold(0.0f32, f32::max)
}

fn validate_waveform_similarity(
    original_frames: &[AudioFrame],
    decoded_frames: &[AudioFrame],
) -> Result<()> {
    let original_rms = calculate_rms(original_frames);
    let decoded_rms = calculate_rms(decoded_frames);

    let rms_error = ((decoded_rms - original_rms).abs() / original_rms) * 100.0;

    println!("Original RMS: {:.6}", original_rms);
    println!("Decoded RMS: {:.6}", decoded_rms);
    println!("RMS Error: {:.2}%", rms_error);

    assert!(
        rms_error < 20.0,
        "RMS error {:.2}% exceeds threshold (20%)",
        rms_error
    );

    let original_peak = calculate_peak(original_frames);
    let decoded_peak = calculate_peak(decoded_frames);

    println!("Original Peak: {:.6}", original_peak);
    println!("Decoded Peak: {:.6}", decoded_peak);

    let peak_error = ((decoded_peak - original_peak).abs() / original_peak) * 100.0;
    assert!(
        peak_error < 30.0,
        "Peak error {:.2}% exceeds threshold (30%)",
        peak_error
    );

    Ok(())
}

fn save_decoded_audio_as_wav(frames: &[AudioFrame], filename: &str) -> Result<()> {
    let artifacts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let full_filename = format!("{}_{}.wav", filename, timestamp);
    let filepath = artifacts_dir.join(&full_filename);

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(&filepath, spec)?;

    for frame in frames {
        for sample in &frame.samples {
            writer.write_sample(*sample)?;
        }
    }

    writer.finalize()?;

    println!("Decoded audio saved: {}", filepath.display());
    Ok(())
}

#[test]
fn test_encode_sine_wave_basic() -> Result<()> {
    let config = SineWaveConfig {
        frequency: 440.0,
        amplitude: 0.5,
        duration_secs: 1.0,
    };

    let frames = generate_sine_wave(config);
    let mut encoder = OpusEncoderWrapper::new(48000, 2)?;
    let mut encoded_buffer = vec![0u8; 4000];

    for (i, frame) in frames.iter().enumerate() {
        let encoded_len = encoder.encode_float(&frame.samples, &mut encoded_buffer)?;
        println!("Frame {}: encoded {} bytes", i, encoded_len);

        assert!(
            encoded_len >= 20 && encoded_len <= 500,
            "Encoded size {} is out of expected range (20-500 bytes)",
            encoded_len
        );
    }

    Ok(())
}

#[test]
fn test_encode_decode_roundtrip() -> Result<()> {
    init_tracing();

    let config = SineWaveConfig {
        frequency: 1000.0,
        amplitude: 0.5,
        duration_secs: 2.0,
    };

    let original_frames = generate_sine_wave(config);
    println!(
        "Generated {} frames of 1000Hz sine wave",
        original_frames.len()
    );

    let mut encoder = OpusEncoderWrapper::new(48000, 2)?;
    let mut decoder = OpusDecoderWrapper::new(48000, 2)?;

    let mut encoded_buffer = vec![0u8; 4000];
    let mut decoded_frames = Vec::new();

    for (i, frame) in original_frames.iter().enumerate() {
        let encoded_len = encoder.encode_float(&frame.samples, &mut encoded_buffer)?;
        println!("Frame {}: encoded {} bytes", i, encoded_len);

        let mut decoded_buffer = vec![0f32; SAMPLES_PER_FRAME * 2];
        let decoded_len =
            decoder.decode_float(&encoded_buffer[..encoded_len], &mut decoded_buffer)?;

        decoded_frames.push(AudioFrame {
            samples: decoded_buffer[..decoded_len].to_vec(),
            sample_rate: 48000,
            channels: 2,
            timestamp_us: frame.timestamp_us,
        });
    }

    println!("Decoded {} frames", decoded_frames.len());

    save_decoded_audio_as_wav(&decoded_frames, "sine_1000hz")?;
    validate_waveform_similarity(&original_frames, &decoded_frames)?;

    println!("✓ Encode-decode roundtrip successful");

    Ok(())
}

#[test]
fn test_encode_multiple_frequencies() -> Result<()> {
    let frequencies = vec![440.0, 1000.0, 2000.0];

    for freq in frequencies {
        println!("\n=== Testing {}Hz ===", freq);

        let config = SineWaveConfig {
            frequency: freq,
            amplitude: 0.5,
            duration_secs: 1.0,
        };

        let frames = generate_sine_wave(config);

        let mut encoder = OpusEncoderWrapper::new(48000, 2)?;
        let mut decoder = OpusDecoderWrapper::new(48000, 2)?;
        let mut encoded_buffer = vec![0u8; 4000];
        let mut decoded_frames = Vec::new();

        let mut total_bytes = 0;

        for frame in &frames {
            let encoded_len = encoder.encode_float(&frame.samples, &mut encoded_buffer)?;
            total_bytes += encoded_len;

            let mut decoded_buffer = vec![0f32; SAMPLES_PER_FRAME * 2];
            let decoded_len =
                decoder.decode_float(&encoded_buffer[..encoded_len], &mut decoded_buffer)?;

            decoded_frames.push(AudioFrame {
                samples: decoded_buffer[..decoded_len].to_vec(),
                sample_rate: 48000,
                channels: 2,
                timestamp_us: frame.timestamp_us,
            });
        }

        println!(
            "{}Hz: {} frames, {} total bytes, {} avg bytes/frame",
            freq,
            frames.len(),
            total_bytes,
            total_bytes / frames.len()
        );

        let filename = format!("sine_{}hz", freq as u32);
        save_decoded_audio_as_wav(&decoded_frames, &filename)?;
        validate_waveform_similarity(&frames, &decoded_frames)?;
    }

    Ok(())
}

#[tokio::test]
async fn test_opus_encoder_factory() -> Result<()> {
    init_tracing();

    let factory = OpusEncoderFactory::new();
    let (frame_tx, mut result_rx) = factory.setup();

    let config = SineWaveConfig {
        frequency: 440.0,
        amplitude: 0.5,
        duration_secs: 1.0,
    };
    let frames = generate_sine_wave(config);

    println!("Sending {} frames to encoder", frames.len());

    for frame in frames {
        frame_tx.send(frame).await?;
    }

    drop(frame_tx);

    let mut encoded_results = Vec::new();
    while let Some(result) = result_rx.recv().await {
        println!(
            "Received encoded result: {} bytes",
            result.encoded_data.len()
        );

        assert!(
            result.encoded_data.len() >= 20 && result.encoded_data.len() <= 500,
            "Encoded size is out of range"
        );
        assert_eq!(result.duration, Duration::from_millis(10));

        encoded_results.push(result);
    }

    println!("✓ Received {} encoded results", encoded_results.len());

    Ok(())
}
