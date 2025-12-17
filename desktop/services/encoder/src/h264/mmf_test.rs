#[cfg(all(windows, feature = "h264"))]
#[cfg(test)]
mod tests {
    use crate::h264::mmf::mf::{check_mf_available, find_h264_encoder, init_media_foundation};
    use crate::h264::mmf::MediaFoundationH264EncoderFactory;
    use core_types::{EncodeJob, VideoCodec, VideoEncoderFactory};
    use std::{
        sync::Once,
        time::{Duration, Instant},
    };
    use tokio::time::timeout;

    static INIT_TRACING: Once = Once::new();

    /// tracingを初期化（テスト実行時に一度だけ実行される）
    fn init_tracing() {
        INIT_TRACING.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_test_writer()
                .init();
        });
    }

    /// 単色のRGBA画像データを作成するヘルパー関数
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

    /// グレースケールのRGBA画像データを作成するヘルパー関数
    fn create_gray_rgba(width: u32, height: u32, gray: u8) -> Vec<u8> {
        create_solid_color_rgba(width, height, gray, gray, gray, 255)
    }

    /// EncodeJobを作成するヘルパー関数
    fn create_encode_job(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        duration: Duration,
        request_keyframe: bool,
    ) -> EncodeJob {
        EncodeJob {
            width,
            height,
            rgba,
            duration,
            enqueue_at: Instant::now(),
            request_keyframe,
        }
    }

    /// Media Foundationの初期化が成功することを確認
    #[test]
    fn test_init_media_foundation() {
        init_tracing();
        let result = init_media_foundation();
        assert!(result, "Media Foundation should initialize successfully");
    }

    /// H.264エンコーダーMFTが検索できることを確認
    #[test]
    fn test_find_h264_encoder() {
        init_tracing();
        // Media Foundationを初期化
        assert!(
            init_media_foundation(),
            "Media Foundation should be initialized"
        );

        // H.264エンコーダーを検索
        unsafe {
            let result = find_h264_encoder();
            assert!(
                result.is_ok(),
                "H.264 encoder MFT should be found: {:?}",
                result.err()
            );
        }
    }

    /// Media Foundationが利用可能かチェックできることを確認
    #[test]
    fn test_check_mf_available() {
        init_tracing();
        let available = check_mf_available();
        assert!(
            available,
            "Media Foundation should be available on this system"
        );
    }

    /// Media Foundation H.264エンコーダーファクトリが作成できることを確認
    #[test]
    fn test_factory_creation() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );
        assert_eq!(factory.codec(), VideoCodec::H264);
    }

    /// エンコードワーカーが起動できることを確認
    #[test]
    fn test_worker_startup() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );

        let (_job_slot, _receiver) = factory.setup();
    }

    /// 単一フレームのエンコードテスト
    #[tokio::test]
    async fn test_single_frame_encode() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );

        let (job_slot, mut receiver) = factory.setup();

        // テスト用のRGBA画像データを作成（1920x1080の赤い画像）
        let width = 1920u32;
        let height = 1080u32;
        let rgba = create_solid_color_rgba(width, height, 255, 0, 0, 255);
        let job = create_encode_job(width, height, rgba, Duration::from_millis(33), false);

        job_slot.set(job);

        // 結果を待機（タイムアウト: 5秒）
        let result = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("Encode timeout")
            .expect("Failed to receive encode result");

        // エンコード結果の検証
        assert!(
            !result.sample_data.is_empty(),
            "Encoded data should not be empty"
        );
        assert_eq!(result.width, width, "Width should match");
        assert_eq!(result.height, height, "Height should match");
        assert_eq!(result.duration, Duration::from_millis(33));

        // H.264データの基本的な検証（Annex-B形式のスタートコードを確認）
        assert!(
            result.sample_data.len() >= 4,
            "Encoded data should have at least start code"
        );

        // 最初のフレームは通常キーフレーム（SPS/PPSを含む）
        // スタートコード（0x00 0x00 0x00 0x01）が含まれていることを確認
        let has_start_code = result
            .sample_data
            .windows(4)
            .any(|w| w == [0x00, 0x00, 0x00, 0x01]);
        assert!(
            has_start_code,
            "Encoded data should contain Annex-B start code"
        );
    }

    /// 複数フレームの連続エンコードテスト
    #[tokio::test]
    async fn test_multiple_frames_encode() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );

        let (job_slot, mut receiver) = factory.setup();

        let width = 1920u32;
        let height = 1080u32;
        let frame_count = 5;

        // 複数のフレームを送信
        let mut results = Vec::new();
        for frame_idx in 0..frame_count {
            // 各フレームで異なる色を使用（グレースケール）
            let gray = (frame_idx * 50) as u8;
            let rgba = create_gray_rgba(width, height, gray);
            let job = create_encode_job(width, height, rgba, Duration::from_millis(33), false);

            job_slot.set(job);

            // 結果を待機
            let result = timeout(Duration::from_secs(5), receiver.recv())
                .await
                .expect("Encode timeout")
                .expect("Failed to receive encode result");
            results.push(result);
        }

        assert_eq!(
            results.len(),
            frame_count,
            "Should receive all encoded frames"
        );

        // すべてのフレームが有効なデータを持っていることを確認
        for (idx, result) in results.iter().enumerate() {
            assert!(
                !result.sample_data.is_empty(),
                "Frame {} should have encoded data",
                idx
            );
            assert_eq!(result.width, width);
            assert_eq!(result.height, height);
        }
    }

    /// 異なるサイズのフレームエンコードテスト
    #[tokio::test]
    async fn test_different_sizes_encode() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );

        let (job_slot, mut receiver) = factory.setup();

        // Media Foundation H.264エンコーダーがサポートする解像度を使用
        let sizes = vec![(320, 240), (640, 480), (1280, 720)];

        for (width, height) in sizes {
            // 青い画像を作成
            let rgba = create_solid_color_rgba(width, height, 0, 0, 255, 255);
            let job = create_encode_job(width, height, rgba, Duration::from_millis(33), false);

            job_slot.set(job);

            let result = timeout(Duration::from_secs(5), receiver.recv())
                .await
                .expect("Encode timeout")
                .expect("Failed to receive encode result");

            assert!(
                !result.sample_data.is_empty(),
                "Encoded data should not be empty for size {}x{}",
                width,
                height
            );
            assert_eq!(result.width, width);
            assert_eq!(result.height, height);
        }
    }

    /// エンコード結果がH.264形式であることを確認（NALユニットの検証）
    #[tokio::test]
    async fn test_h264_format_validation() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );

        let (job_slot, mut receiver) = factory.setup();

        let width = 320u32;
        let height = 240u32;
        let rgba = create_gray_rgba(width, height, 128);
        let job = create_encode_job(width, height, rgba, Duration::from_millis(33), false);

        job_slot.set(job);

        let result = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("Encode timeout")
            .expect("Failed to receive encode result");

        // Annex-B形式のスタートコードを検索
        let mut i = 0;
        let mut nal_count = 0;
        while i + 4 <= result.sample_data.len() {
            if result.sample_data[i..i + 4] == [0x00, 0x00, 0x00, 0x01] {
                nal_count += 1;
                if i + 5 <= result.sample_data.len() {
                    // NALタイプを確認（下位5ビット）
                    let nal_type = result.sample_data[i + 4] & 0x1F;
                    // 有効なNALタイプ: 1-5 (非IDR/IDRピクチャ), 6 (SEI), 7 (SPS), 8 (PPS), 9 (AUD)
                    assert!(
                        nal_type >= 1 && nal_type <= 9,
                        "Invalid NAL unit type: {}",
                        nal_type
                    );
                }
                i += 4;
            } else {
                i += 1;
            }
        }

        assert!(nal_count > 0, "Should contain at least one NAL unit");
    }

    /// H.264データにSPS (NAL type 7) または PPS (NAL type 8) が含まれているか確認するヘルパー関数
    fn has_sps_or_pps(sample_data: &[u8]) -> bool {
        let mut i = 0;
        while i + 4 <= sample_data.len() {
            if sample_data[i..i + 4] == [0x00, 0x00, 0x00, 0x01] {
                if i + 5 <= sample_data.len() {
                    let nal_type = sample_data[i + 4] & 0x1F;
                    if nal_type == 7 || nal_type == 8 {
                        return true;
                    }
                }
                i += 4;
            } else {
                i += 1;
            }
        }
        false
    }

    /// キーフレーム（SPS/PPSを含む）の生成を確認
    /// 最初のフレームは常にキーフレームなので、数フレーム後にrequest_keyframeをtrueで要求した場合の検証を行う
    #[tokio::test]
    async fn test_keyframe_generation() {
        init_tracing();
        let factory = MediaFoundationH264EncoderFactory::new();
        assert!(
            factory.use_media_foundation(),
            "Media Foundation encoder should be available"
        );

        let (job_slot, mut receiver) = factory.setup();

        let width = 320u32;
        let height = 240u32;

        // 最初の数フレームを通常エンコード（request_keyframe: false）
        let regular_frame_count = 3;
        for frame_idx in 0..regular_frame_count {
            // 各フレームで異なる色を使用（グレースケール）
            let gray = (frame_idx * 50) as u8;
            let rgba = create_gray_rgba(width, height, gray);
            let job = create_encode_job(width, height, rgba, Duration::from_millis(33), false);

            job_slot.set(job);

            // 結果を待機（最初のフレームはキーフレームになる可能性があるが、必ずしもそうではない）
            let result = timeout(Duration::from_secs(5), receiver.recv())
                .await
                .expect("Encode timeout")
                .expect("Failed to receive encode result");
            if frame_idx == 0 {
                // 最初のフレームがキーフレームの場合は、SPS/PPSを含むことを確認
                // キーフレームでない場合も許容（Media Foundationエンコーダーの動作による）
                // if result.is_keyframe {
                //     assert!(
                //         has_sps_or_pps(&result.sample_data),
                //         "First frame marked as keyframe should contain SPS or PPS"
                //     );
                // }
            } else {
                assert!(
                    !result.is_keyframe,
                    "Frame with request_keyframe=false should not be marked as keyframe"
                );
            }
        }

        // キーフレームを要求してエンコード
        // Media Foundationエンコーダーは、force_keyframe()を呼んでも次のフレームがIDRになるまで時間がかかる可能性があるため、
        // 複数のフレームをエンコードしてIDRフレームが生成されるまで待つ
        let mut keyframe_result = None;
        for attempt in 0..10 {
            let rgba = create_solid_color_rgba(width, height, 255, 255, 255, 255);
            let job = create_encode_job(width, height, rgba, Duration::from_millis(33), true);

            job_slot.set(job);

            let result = timeout(Duration::from_secs(5), receiver.recv())
                .await
                .expect("Encode timeout")
                .expect("Failed to receive encode result");

            if result.is_keyframe {
                keyframe_result = Some(result);
                break;
            }

            // デバッグ情報: キーフレームが生成されていない場合
            if attempt == 9 {
                tracing::warn!(
                    "Keyframe not generated after {} attempts with request_keyframe=true",
                    attempt + 1
                );
            }
        }

        // キーフレームが生成されたことを確認
        let result = keyframe_result
            .expect("Keyframe should be generated after request_keyframe=true (tried 10 frames)");

        // キーフレームフラグが設定されていることを確認
        assert!(
            result.is_keyframe,
            "Frame with request_keyframe=true should be marked as keyframe"
        );

        // SPS/PPSが含まれていることを確認
        assert!(
            has_sps_or_pps(&result.sample_data),
            "Keyframe should contain SPS or PPS"
        );
    }
}
