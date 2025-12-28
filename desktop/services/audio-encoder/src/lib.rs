use anyhow::Result;
use core_types::{AudioEncoderFactory, AudioEncodeResult, AudioFrame};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Opus エンコーダーの Rust ラッパー
pub struct OpusEncoderWrapper {
    encoder: *mut opus_sys::OpusEncoder,
}

impl OpusEncoderWrapper {
    /// 新しいエンコーダーを作成
    pub fn new(sample_rate: i32, channels: i32) -> Result<Self> {
        let mut error: i32 = 0;
        let encoder = unsafe {
            opus_sys::opus_encoder_create(
                sample_rate,
                channels,
                opus_sys::OPUS_APPLICATION_AUDIO as i32,
                &mut error as *mut i32,
            )
        };

        if error != opus_sys::OPUS_OK as i32 || encoder.is_null() {
            return Err(anyhow::anyhow!("Failed to create Opus encoder: error {}", error));
        }

        Ok(Self { encoder })
    }

    /// ビットレートを設定（TODO: 実装が必要）
    pub fn set_bitrate(&mut self, _bitrate: i32) -> Result<()> {
        // wrapper 関数が bindgen で正しく生成されないため、一旦デフォルト値を使用
        Ok(())
    }

    /// f32 サンプルをエンコード
    pub fn encode_float(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize> {
        let frame_size = (pcm.len() / 2) as i32; // ステレオなので /2
        let encoded_len = unsafe {
            opus_sys::opus_encode_float(
                self.encoder,
                pcm.as_ptr(),
                frame_size,
                output.as_mut_ptr(),
                output.len() as i32,
            )
        };

        if encoded_len < 0 {
            return Err(anyhow::anyhow!("Encoding failed: error {}", encoded_len));
        }

        Ok(encoded_len as usize)
    }
}

impl Drop for OpusEncoderWrapper {
    fn drop(&mut self) {
        unsafe {
            opus_sys::opus_encoder_destroy(self.encoder);
        }
    }
}

unsafe impl Send for OpusEncoderWrapper {}

/// Opus エンコーダーファクトリ
pub struct OpusEncoderFactory;

impl OpusEncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl AudioEncoderFactory for OpusEncoderFactory {
    fn setup(&self) -> (tokio::sync::mpsc::Sender<AudioFrame>, tokio::sync::mpsc::UnboundedReceiver<AudioEncodeResult>) {
        let (frame_tx, mut frame_rx) = mpsc::channel::<AudioFrame>(100);
        let (result_tx, result_rx) = mpsc::unbounded_channel::<AudioEncodeResult>();

        tokio::spawn(async move {
            info!("Opus encoder worker started");

            // エンコーダーを初期化
            let mut encoder = match OpusEncoderWrapper::new(48000, 2) {
                Ok(enc) => enc,
                Err(e) => {
                    error!("Failed to create Opus encoder: {}", e);
                    return;
                }
            };

            // ビットレートを設定（64kbps） - TODO: 実装が必要
            if let Err(e) = encoder.set_bitrate(64000) {
                warn!("Failed to set Opus bitrate: {}", e);
            }

            let mut encoded_buffer = vec![0u8; 4000];

            loop {
                match frame_rx.recv().await {
                    Some(frame) => {
                        // フレームをエンコード（f32 サンプルを直接エンコード）
                        let encoded_len = match encoder.encode_float(&frame.samples, &mut encoded_buffer) {
                            Ok(len) => len,
                            Err(e) => {
                                error!("Failed to encode audio frame: {}", e);
                                continue;
                            }
                        };

                        // エンコード結果を送信
                        let result = AudioEncodeResult {
                            encoded_data: encoded_buffer[..encoded_len].to_vec(),
                            duration: Duration::from_millis(10), // 10msフレーム
                        };

                        if let Err(e) = result_tx.send(result) {
                            error!("Failed to send encode result: {}", e);
                            break;
                        }

                        debug!("Encoded audio frame: {} bytes", encoded_len);
                    }
                    None => {
                        debug!("Audio frame channel closed");
                        break;
                    }
                }
            }

            info!("Opus encoder worker stopped");
        });

        (frame_tx, result_rx)
    }
}
