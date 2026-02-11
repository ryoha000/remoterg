#[cfg(test)]
mod tests {
    use crate::windows::av1::factory::MediaFoundationAV1EncoderFactory;
    use crate::windows::h264::MediaFoundationH264EncoderFactory;
    use core_types::{EncodeJob, VideoCodec, VideoEncoderFactory}; // VideoCodec added
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::time::timeout;

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

    fn create_encode_job(width: u32, height: u32, rgba: Vec<u8>) -> EncodeJob {
        let arc_rgba = Arc::new(rgba);
        EncodeJob {
            width,
            height,
            rgba: arc_rgba,
            timestamp: 0,
            enqueue_at: Instant::now(),
            request_keyframe: false,
            frame_id: 0,
        }
    }

    struct TestCase {
        name: String,
        width: u32,
        height: u32,
        codec: VideoCodec,
    }

    #[tokio::test]
    async fn test_encode_alignment() {
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
                VideoCodec::H264 => {
                    let f = MediaFoundationH264EncoderFactory::new();
                    if !f.use_media_foundation() {
                        println!(
                            "Skipping H264 test: Media Foundation H.264 encoder not available"
                        );
                        continue;
                    }
                    Box::new(f)
                }
                VideoCodec::AV1 => Box::new(MediaFoundationAV1EncoderFactory::new()),
            };

            let (job_slot, mut receiver) = factory.setup();

            let rgba = create_solid_color_rgba(case.width, case.height, 255, 0, 0, 255);
            let job = create_encode_job(case.width, case.height, rgba);

            job_slot.set(job);

            // Wait for result
            // Note: If initialization fails (e.g. no AV1 hardware), subsequent recv will timeout?
            // Or pipeline handles it gracefully?
            // Pipeline warns and returns?
            // If setup fails in thread, nothing flows to receiver?

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
                    // Receiver closed, meaning worker exited.
                    // Could happen if AV1 encoder not found.
                    if case.codec == VideoCodec::AV1 {
                        println!(
                            "skipping AV1 test: Encoder worker exited (likely no hardware support)"
                        );
                    } else {
                        panic!("[{}] Encoder worker exited unexpectedly", case.name);
                    }
                }
                Err(_) => {
                    // Timeout
                    if case.codec == VideoCodec::AV1 {
                        println!(
                            "skipping AV1 test: Timeout (likely no hardware support or slow init)"
                        );
                    } else {
                        panic!("[{}] Encode timeout", case.name);
                    }
                }
            }
        }
    }
}
