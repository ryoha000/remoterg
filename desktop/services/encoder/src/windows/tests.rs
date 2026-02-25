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

    struct KeyframeTestCase {
        name: String,
        codec: VideoCodec,
    }

    #[tokio::test]
    async fn test_keyframe_forcing() {
        init_tracing();
        let test_cases = vec![
            KeyframeTestCase {
                name: "H264 Keyframe Forcing".to_string(),
                codec: VideoCodec::H264,
            },
            KeyframeTestCase {
                name: "AV1 Keyframe Forcing".to_string(),
                codec: VideoCodec::AV1,
            },
        ];

        for case in test_cases {
            println!("Running keyframe test case: {}", case.name);

            let factory: Box<dyn VideoEncoderFactory> = match case.codec {
                VideoCodec::H264 => Box::new(MediaFoundationH264EncoderFactory::new()),
                VideoCodec::AV1 => Box::new(MediaFoundationAV1EncoderFactory::new()),
            };

            let (job_slot, mut receiver) = factory.setup();
            let width = 1280;
            let height = 720;
            let rgba = create_solid_color_rgba(width, height, 0, 255, 0, 255);
            let (mut job, _device, _texture) = create_encode_job(width, height, rgba);

            // 1. Initial frame naturally becomes a keyframe (or because it's the first)
            job.frame_id = 1;
            job.request_keyframe = false;
            job_slot.set(job);
            
            let result1 = timeout(Duration::from_secs(5), receiver.recv()).await.expect("Timeout on frame 1").expect("Encoder exited");
            // Some encoders might not mark the very first frame explicitly or they do.
            println!("[{}] Frame 1 keyframe: {}", case.name, result1.is_keyframe);
            
            // 2. Second frame without request_keyframe should be a P-frame (not a keyframe)
            let rgba2 = create_solid_color_rgba(width, height, 0, 255, 0, 255);
            let (mut job2, _device2, _texture2) = create_encode_job(width, height, rgba2);
            job2.frame_id = 2;
            job2.request_keyframe = false;
            job_slot.set(job2);

            let result2 = timeout(Duration::from_secs(5), receiver.recv()).await.expect("Timeout on frame 2").expect("Encoder exited");
            assert!(!result2.is_keyframe, "[{}] Frame 2 should not be a keyframe", case.name);

            // 3. Third frame with request_keyframe = true MUST be a keyframe
            let rgba3 = create_solid_color_rgba(width, height, 0, 0, 255, 255);
            let (mut job3, _device3, _texture3) = create_encode_job(width, height, rgba3);
            job3.frame_id = 3;
            job3.request_keyframe = true;
            job_slot.set(job3);

            let result3 = timeout(Duration::from_secs(5), receiver.recv()).await.expect("Timeout on frame 3").expect("Encoder exited");
            assert!(result3.is_keyframe, "[{}] Frame 3 MUST be a keyframe due to request_keyframe=true", case.name);
        }
    }
}
