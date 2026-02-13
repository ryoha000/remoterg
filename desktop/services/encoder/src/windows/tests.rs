#[cfg(test)]
mod tests {
    use crate::windows::av1::factory::MediaFoundationAV1EncoderFactory;
    use crate::windows::h264::MediaFoundationH264EncoderFactory;
    use core_types::{EncodeJob, VideoCodec, VideoEncoderFactory}; // VideoCodec added
    use std::sync::Once;
    use std::time::{Duration, Instant};
    use tokio::time::timeout;

    static INIT_TRACING: Once = Once::new();

    /// tracingを初期化（テスト実行時に一度だけ実行される）
    fn init_tracing() {
        INIT_TRACING.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_test_writer()
                .init();
        });
    }

    fn create_solid_color_rgba(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(a);
        }
        rgba
    }

    fn create_encode_job(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> (
        EncodeJob,
        gpu_texture::D3D11Device,
        gpu_texture::SharedTexture,
    ) {
        // デバイスを作成（テストのライフタイム全体で保持する必要がある）
        let device = gpu_texture::D3D11Device::new().expect("Failed to create D3D11 device");

        // テクスチャを作成（テストのライフタイム全体で保持する必要がある）
        let texture = gpu_texture::SharedTexture::from_rgba(&device, &rgba, width, height)
            .expect("Failed to create shared texture from RGBA data");

        let texture_handle = texture.handle();

        let job = EncodeJob {
            width,
            height,
            timestamp: 0,
            enqueue_at: Instant::now(),
            request_keyframe: false,
            frame_id: 0,
            texture_handle: Some(texture_handle),
        };

        (job, device, texture)
    }

    struct TestCase {
        name: String,
        width: u32,
        height: u32,
        codec: VideoCodec,
    }

    #[tokio::test]
    async fn test_encode_alignment() {
        init_tracing();
        let test_cases = vec![
            TestCase {
                name: "H264 Full HD".to_string(),
                width: 1920,
                height: 1080,
                codec: VideoCodec::H264,
            },
            TestCase {
                name: "H264 Odd Resolution".to_string(),
                width: 1919,
                height: 1079,
                codec: VideoCodec::H264,
            },
            TestCase {
                name: "AV1 Full HD".to_string(),
                width: 1920,
                height: 1080,
                codec: VideoCodec::AV1,
            },
            TestCase {
                name: "AV1 Odd Resolution".to_string(),
                width: 1919,
                height: 1079,
                codec: VideoCodec::AV1,
            },
        ];

        for case in test_cases {
            println!("Running test case: {}", case.name);

            let factory: Box<dyn VideoEncoderFactory> = match case.codec {
                VideoCodec::H264 => Box::new(MediaFoundationH264EncoderFactory::new()),
                VideoCodec::AV1 => Box::new(MediaFoundationAV1EncoderFactory::new()),
            };

            let (job_slot, mut receiver) = factory.setup();

            let rgba = create_solid_color_rgba(case.width, case.height, 255, 0, 0, 255);
            let (job, _device, _texture) = create_encode_job(case.width, case.height, rgba);
            // _deviceと_textureはこのスコープが終わるまで保持される（重要！）

            job_slot.set(job);

            // Wait for result

            let result_opt = timeout(Duration::from_secs(5), receiver.recv()).await;

            match result_opt {
                Ok(Some(result)) => {
                    assert!(
                        !result.sample_data.is_empty(),
                        "[{}] Encoded data should not be empty",
                        case.name
                    );
                    println!(
                        "[{}] Encoded frame size: {}x{}",
                        case.name, result.width, result.height
                    );

                    // Check alignment
                    let expected_width = (case.width / 2) * 2;
                    let expected_height = (case.height / 2) * 2;
                    assert_eq!(
                        result.width, expected_width,
                        "[{}] Width mismatch",
                        case.name
                    );
                    assert_eq!(
                        result.height, expected_height,
                        "[{}] Height mismatch",
                        case.name
                    );
                }
                Ok(None) => {
                    panic!("[{}] Encoder worker exited unexpectedly", case.name);
                }
                Err(_) => {
                    // Timeout
                    panic!("[{}] Encode timeout", case.name);
                }
            }
        }
    }
}
