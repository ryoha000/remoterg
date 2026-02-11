#[cfg(test)]
mod tests {
    use crate::h264::mmf::MediaFoundationH264EncoderFactory;
    use core_types::{EncodeJob, VideoEncoderFactory};
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

    fn create_encode_job(
        width: u32,
        height: u32,
        rgba: Vec<u8>
    ) -> EncodeJob {
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

    #[tokio::test]
    async fn test_encode_alignment_full_hd() {
        let factory = MediaFoundationH264EncoderFactory::new();
        if !factory.use_media_foundation() {
            println!("Skipping test: Media Foundation H.264 encoder not available");
            return;
        }

        let (job_slot, mut receiver) = factory.setup();

        let width = 1920;
        let height = 1080;
        let rgba = create_solid_color_rgba(width, height, 255, 0, 0, 255);
        let job = create_encode_job(width, height, rgba);

        job_slot.set(job);

        let result = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("Encode timeout")
            .expect("Failed to receive encode result");

        assert!(!result.sample_data.is_empty(), "Encoded data should not be empty");
        println!("Full HD Encoded frame size: {}x{}", result.width, result.height);
        assert_eq!(result.width, width);
        assert_eq!(result.height, height);
    }

    #[tokio::test]
    async fn test_encode_alignment_odd_resolution() {
        let factory = MediaFoundationH264EncoderFactory::new();
        if !factory.use_media_foundation() {
            println!("Skipping test: Media Foundation H.264 encoder not available");
            return;
        }

        let (job_slot, mut receiver) = factory.setup();

        // Odd resolution
        let width = 1919;
        let height = 1079;
        let rgba = create_solid_color_rgba(width, height, 0, 255, 0, 255);
        let job = create_encode_job(width, height, rgba);

        job_slot.set(job);

        let result = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("Encode timeout")
            .expect("Failed to receive encode result");

        assert!(!result.sample_data.is_empty(), "Encoded data should not be empty");
        println!("Odd Resolution Encoded frame size: {}x{}", result.width, result.height);
        
        // Factory aligns to even numbers (truncates)
        let expected_width = (width / 2) * 2;
        let expected_height = (height / 2) * 2;
        assert_eq!(result.width, expected_width);
        assert_eq!(result.height, expected_height);
    }
}
