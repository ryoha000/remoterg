#[cfg(windows)]
pub mod mf {
    use anyhow::{Context, Result};
    use std::mem::ManuallyDrop;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc as tokio_mpsc;
    use tracing::{info, warn};
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFTransform, MFCreateMediaType, MFCreateSample, MFStartup, MFTEnumEx,
        MFVideoFormat_H264, MFSTARTUP_FULL,
    };

    use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

    // Media Foundationの初期化状態を管理（スレッドセーフ）
    static MF_INITIALIZED: AtomicBool = AtomicBool::new(false);

    /// Media Foundation H.264 エンコーダーファクトリ
    /// 利用可能でない場合はOpenH264にフォールバック
    pub struct MediaFoundationH264EncoderFactory {
        use_mf: bool,
    }

    impl MediaFoundationH264EncoderFactory {
        pub fn new() -> Self {
            // Media Foundationが利用可能かチェック
            let use_mf = check_mf_available();
            if use_mf {
                info!("Media Foundation H.264 encoder is available, using MF encoder");
            } else {
                warn!("Media Foundation H.264 encoder is not available, will fallback to OpenH264");
            }
            Self { use_mf }
        }

        pub fn use_media_foundation(&self) -> bool {
            self.use_mf
        }
    }

    impl VideoEncoderFactory for MediaFoundationH264EncoderFactory {
        fn start_workers(
            &self,
        ) -> (
            Vec<std::sync::mpsc::Sender<EncodeJob>>,
            tokio_mpsc::UnboundedReceiver<EncodeResult>,
        ) {
            if self.use_mf {
                start_mf_encode_workers()
            } else {
                // OpenH264にフォールバック
                crate::openh264::start_encode_workers()
            }
        }

        fn codec(&self) -> VideoCodec {
            VideoCodec::H264
        }
    }

    /// Media Foundationが利用可能かチェック
    fn check_mf_available() -> bool {
        // Media Foundationの初期化を試行
        if !init_media_foundation() {
            return false;
        }

        // H.264エンコーダーMFTが存在するか確認
        unsafe {
            match find_h264_encoder() {
                Ok(_) => true,
                Err(e) => {
                    warn!("H.264 encoder MFT not found: {}", e);
                    false
                }
            }
        }
    }

    /// Media Foundationを初期化（スレッドセーフ）
    fn init_media_foundation() -> bool {
        if MF_INITIALIZED.load(Ordering::Acquire) {
            return true;
        }

        unsafe {
            match MFStartup(MFSTARTUP_FULL, 0) {
                Ok(_) => {
                    MF_INITIALIZED.store(true, Ordering::Release);
                    true
                }
                Err(e) => {
                    warn!("Failed to initialize Media Foundation: {}", e);
                    false
                }
            }
        }
    }

    /// H.264エンコーダーMFTを検索
    unsafe fn find_h264_encoder() -> Result<()> {
        use windows::Win32::Media::MediaFoundation::{
            MFMediaType_Video, MFVideoFormat_NV12, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO,
        };

        let input_type = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };

        let output_type = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };

        let activate_array: *mut *mut Option<IMFActivate> = ptr::null_mut();
        let mut count: u32 = 0;

        MFTEnumEx(
            windows::core::GUID::zeroed(), // guidCategory
            MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SYNCMFT.0),
            Some(&input_type as *const _),
            Some(&output_type as *const _),
            activate_array,
            &mut count,
        )
        .ok()
        .context("Failed to enumerate H.264 encoder MFT")?;

        if count == 0 {
            return Err(anyhow::anyhow!("No H.264 encoder MFT found"));
        }

        // リソースをクリーンアップ
        if !activate_array.is_null() {
            for i in 0..count {
                let activate_ptr_ptr = activate_array.add(i as usize);
                if !activate_ptr_ptr.is_null() {
                    unsafe {
                        let activate_ptr = std::ptr::read(activate_ptr_ptr);
                        if !activate_ptr.is_null() {
                            if let Some(activate) = std::ptr::read(activate_ptr) {
                                let _ = activate.ShutdownObject();
                            }
                        }
                    }
                }
            }
            windows::Win32::System::Com::CoTaskMemFree(Some(activate_array as *mut _));
        }

        Ok(())
    }

    /// RGBA形式の画像データをNV12形式に変換
    fn rgba_to_nv12(
        rgba: &[u8],
        source_width: u32,
        source_height: u32,
        encode_width: u32,
        encode_height: u32,
    ) -> Vec<u8> {
        let source_w = source_width as usize;
        let source_h = source_height as usize;
        let encode_w = encode_width as usize;
        let encode_h = encode_height as usize;

        // NV12形式: Y平面 + UV平面（インターリーブ）
        let y_plane_size = encode_w * encode_h;
        let uv_plane_size = (encode_w * encode_h) / 2;
        let mut nv12 = vec![0u8; y_plane_size + uv_plane_size];

        // Y平面の変換
        for y in 0..encode_h {
            for x in 0..encode_w {
                let src_y = (y * source_h / encode_h).min(source_h - 1);
                let src_x = (x * source_w / encode_w).min(source_w - 1);
                let src_idx = (src_y * source_w + src_x) * 4;

                if src_idx + 2 < rgba.len() {
                    let r = rgba[src_idx] as f32;
                    let g = rgba[src_idx + 1] as f32;
                    let b = rgba[src_idx + 2] as f32;

                    let y_val = (0.257 * r + 0.504 * g + 0.098 * b + 16.0).round();
                    nv12[y * encode_w + x] = y_val.clamp(0.0, 255.0) as u8;
                }
            }
        }

        // UV平面の変換（インターリーブ形式）
        for y in (0..encode_h).step_by(2) {
            for x in (0..encode_w).step_by(2) {
                let mut u_acc = 0f32;
                let mut v_acc = 0f32;
                let mut sample_count = 0;

                for sy in 0..2 {
                    let src_y = ((y + sy) * source_h / encode_h).min(source_h - 1);
                    for sx in 0..2 {
                        let src_x = ((x + sx) * source_w / encode_w).min(source_w - 1);
                        let src_idx = (src_y * source_w + src_x) * 4;

                        if src_idx + 2 < rgba.len() {
                            let r = rgba[src_idx] as f32;
                            let g = rgba[src_idx + 1] as f32;
                            let b = rgba[src_idx + 2] as f32;

                            u_acc += -0.148 * r - 0.291 * g + 0.439 * b + 128.0;
                            v_acc += 0.439 * r - 0.368 * g - 0.071 * b + 128.0;
                            sample_count += 1;
                        }
                    }
                }

                if sample_count > 0 {
                    let u_val = (u_acc / sample_count as f32).clamp(0.0, 255.0) as u8;
                    let v_val = (v_acc / sample_count as f32).clamp(0.0, 255.0) as u8;
                    let uv_idx = y_plane_size + (y / 2) * encode_w + (x / 2) * 2;
                    if uv_idx + 1 < nv12.len() {
                        nv12[uv_idx] = u_val;
                        nv12[uv_idx + 1] = v_val;
                    }
                }
            }
        }

        nv12
    }

    /// H.264エンコーダー構造体
    struct MfEncoder {
        transform: IMFTransform,
        width: u32,
        height: u32,
    }

    impl MfEncoder {
        /// 新しいエンコーダーを作成
        fn new(width: u32, height: u32) -> Result<Self> {
            use windows::Win32::Media::MediaFoundation::{
                MFMediaType_Video, MFVideoFormat_NV12, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE,
                MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO,
            };

            let input_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_NV12,
            };

            let output_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_H264,
            };

            let activate_array: *mut *mut Option<IMFActivate> = ptr::null_mut();
            let mut count: u32 = 0;

            unsafe {
                MFTEnumEx(
                    windows::core::GUID::zeroed(), // guidCategory
                    MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SYNCMFT.0),
                    Some(&input_type as *const _),
                    Some(&output_type as *const _),
                    activate_array,
                    &mut count,
                )
                .ok()
                .context("Failed to enumerate H.264 encoder MFT")?;

                if count == 0 {
                    return Err(anyhow::anyhow!("No H.264 encoder MFT found"));
                }

                // 最初のMFTを使用
                if activate_array.is_null() {
                    return Err(anyhow::anyhow!("Activate pointer is null"));
                }
                let activate_ptr_ptr = *activate_array;
                if activate_ptr_ptr.is_null() {
                    return Err(anyhow::anyhow!("Activate pointer is null"));
                }
                let activate_option = std::ptr::read(activate_ptr_ptr);
                let activate =
                    activate_option.ok_or_else(|| anyhow::anyhow!("Failed to get activate"))?;

                let transform: IMFTransform = activate
                    .ActivateObject()
                    .ok()
                    .context("Failed to activate H.264 encoder MFT")?;

                // 入力メディアタイプを設定
                let input_media_type = MFCreateMediaType()
                    .ok()
                    .context("Failed to create input media type")?;

                input_media_type
                    .SetGUID(
                        &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                        &MFMediaType_Video,
                    )
                    .ok()
                    .context("Failed to set input major type")?;
                input_media_type
                    .SetGUID(
                        &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                        &MFVideoFormat_NV12,
                    )
                    .ok()
                    .context("Failed to set input subtype")?;

                // フレームサイズを設定（UINT64形式でパック）
                let frame_size = ((width as u64) << 32) | (height as u64);
                input_media_type
                    .SetUINT64(
                        &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                        frame_size,
                    )
                    .ok()
                    .context("Failed to set frame size")?;

                // フレームレートを設定（60fps）
                let frame_rate = (60u64 << 32) | 1u64;
                input_media_type
                    .SetUINT64(
                        &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                        frame_rate,
                    )
                    .ok()
                    .context("Failed to set frame rate")?;

                transform
                    .SetInputType(0, &input_media_type, 0)
                    .ok()
                    .context("Failed to set input type")?;

                // 出力メディアタイプを設定
                let output_media_type = MFCreateMediaType()
                    .ok()
                    .context("Failed to create output media type")?;

                output_media_type
                    .SetGUID(
                        &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                        &MFMediaType_Video,
                    )
                    .ok()
                    .context("Failed to set output major type")?;
                output_media_type
                    .SetGUID(
                        &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                        &MFVideoFormat_H264,
                    )
                    .ok()
                    .context("Failed to set output subtype")?;

                // 出力フレームサイズを設定
                output_media_type
                    .SetUINT64(
                        &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                        frame_size,
                    )
                    .ok()
                    .context("Failed to set output frame size")?;

                transform
                    .SetOutputType(0, &output_media_type, 0)
                    .ok()
                    .context("Failed to set output type")?;

                // ストリーム開始を通知
                transform
                    .ProcessMessage(
                        windows::Win32::Media::MediaFoundation::MFT_MESSAGE_NOTIFY_START_OF_STREAM,
                        0,
                    )
                    .ok()
                    .context("Failed to notify start of stream")?;

                // リソースをクリーンアップ
                for i in 0..count {
                    let activate_ptr_ptr = activate_array.add(i as usize);
                    if !activate_ptr_ptr.is_null() {
                        let activate_ptr = std::ptr::read(activate_ptr_ptr);
                        if !activate_ptr.is_null() {
                            if let Some(activate) = std::ptr::read(activate_ptr) {
                                let _ = activate.ShutdownObject();
                            }
                        }
                    }
                }
                windows::Win32::System::Com::CoTaskMemFree(Some(activate_array as *mut _));

                Ok(Self {
                    transform,
                    width,
                    height,
                })
            }
        }

        /// フレームをエンコード
        fn encode(&mut self, nv12_data: &[u8], timestamp: i64) -> Result<Vec<u8>> {
            use windows::Win32::Media::MediaFoundation::{
                MFCreateMemoryBuffer, MFT_OUTPUT_DATA_BUFFER, MF_E_TRANSFORM_NEED_MORE_INPUT,
                MF_E_TRANSFORM_STREAM_CHANGE,
            };

            unsafe {
                // 入力サンプルを作成
                let sample = MFCreateSample()
                    .ok()
                    .context("Failed to create input sample")?;

                // メモリバッファを作成
                let buffer_size = nv12_data.len() as u32;
                let buffer = MFCreateMemoryBuffer(buffer_size)
                    .ok()
                    .context("Failed to create memory buffer")?;

                // バッファにデータをコピー
                let mut data_ptr: *mut u8 = ptr::null_mut();
                let mut max_length: u32 = 0;
                buffer
                    .Lock(&mut data_ptr, Some(&mut max_length), None)
                    .ok()
                    .context("Failed to lock buffer")?;

                if max_length >= buffer_size {
                    ptr::copy_nonoverlapping(nv12_data.as_ptr(), data_ptr, nv12_data.len());
                }

                buffer.Unlock().ok().context("Failed to unlock buffer")?;
                buffer
                    .SetCurrentLength(buffer_size)
                    .ok()
                    .context("Failed to set buffer length")?;

                // サンプルにバッファを追加
                sample
                    .AddBuffer(&buffer)
                    .ok()
                    .context("Failed to add buffer to sample")?;

                // タイムスタンプを設定
                sample
                    .SetSampleTime(timestamp)
                    .ok()
                    .context("Failed to set sample time")?;

                // 入力サンプルを処理
                self.transform
                    .ProcessInput(0, &sample, 0)
                    .ok()
                    .context("Failed to process input")?;

                // 出力を取得
                let mut output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(None),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                };
                let mut status: u32 = 0;

                let mut encoded_data = Vec::new();
                loop {
                    match self.transform.ProcessOutput(
                        0,
                        std::slice::from_mut(&mut output_data_buffer),
                        &mut status,
                    ) {
                        Ok(_) => {
                            if let Some(output_sample) = output_data_buffer.pSample.take() {
                                // サンプルからバッファを取得
                                let buffer = output_sample
                                    .GetBufferByIndex(0)
                                    .ok()
                                    .context("Failed to get output buffer")?;

                                let mut data_ptr: *mut u8 = ptr::null_mut();
                                let mut max_length: u32 = 0;
                                buffer
                                    .Lock(&mut data_ptr, Some(&mut max_length), None)
                                    .ok()
                                    .context("Failed to lock output buffer")?;

                                let current_length = buffer
                                    .GetCurrentLength()
                                    .ok()
                                    .context("Failed to get output buffer length")?;

                                if current_length > 0 && !data_ptr.is_null() {
                                    let slice = std::slice::from_raw_parts(
                                        data_ptr,
                                        current_length as usize,
                                    );
                                    encoded_data.extend_from_slice(slice);
                                }

                                buffer
                                    .Unlock()
                                    .ok()
                                    .context("Failed to unlock output buffer")?;
                            }
                            break;
                        }
                        Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                            // より多くの入力が必要（通常は発生しない）
                            break;
                        }
                        Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                            // ストリーム変更（通常は発生しない）
                            break;
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("ProcessOutput failed: {}", e));
                        }
                    }
                }

                Ok(encoded_data)
            }
        }
    }

    /// H.264データをAnnex-B形式に変換し、SPS/PPSを検出
    fn annexb_from_mf_data(data: &[u8]) -> (Vec<u8>, bool) {
        const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
        let mut has_sps_pps = false;
        let mut result = Vec::new();

        // Media Foundationの出力は通常AVC形式（NAL長プレフィックス）なので、
        // Annex-B形式（スタートコード）に変換する必要がある
        let mut i = 0;
        while i < data.len() {
            if i + 4 <= data.len() {
                // NAL長を読み取る（ビッグエンディアン）
                let nal_length =
                    u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;

                i += 4;

                if i + nal_length <= data.len() {
                    let nal_unit = &data[i..i + nal_length];

                    // NALタイプをチェック（SPS=7, PPS=8）
                    if !nal_unit.is_empty() {
                        let nal_type = nal_unit[0] & 0x1F;
                        if nal_type == 7 || nal_type == 8 {
                            has_sps_pps = true;
                        }
                    }

                    // スタートコードを追加
                    result.extend_from_slice(START_CODE);
                    result.extend_from_slice(nal_unit);

                    i += nal_length;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        (result, has_sps_pps)
    }

    /// Media Foundationエンコードワーカーを起動
    fn start_mf_encode_workers() -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<EncodeJob>();
        let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        std::thread::spawn(move || {
            let mut width = 0;
            let mut height = 0;
            let mut encoder: Option<MfEncoder> = None;
            let mut encode_failures = 0u32;
            let mut empty_samples = 0u32;
            let mut successful_encodes = 0u32;
            let mut dropped_frames = 0u32;
            let mut frame_timestamp = 0i64;

            // パフォーマンス統計用
            let mut total_rgb_dur = Duration::ZERO;
            let mut total_encode_dur = Duration::ZERO;
            let mut total_pack_dur = Duration::ZERO;
            let mut total_queue_wait_dur = Duration::ZERO;
            let mut last_stats_log = Instant::now();

            const MAX_QUEUE_DRAIN: usize = 10;

            loop {
                let mut job = match job_rx.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                };

                let mut skipped_count = 0;
                loop {
                    match job_rx.try_recv() {
                        Ok(newer_job) => {
                            if skipped_count < MAX_QUEUE_DRAIN {
                                skipped_count += 1;
                                job = newer_job;
                            } else {
                                break;
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    }
                }

                if skipped_count > 0 {
                    dropped_frames += skipped_count as u32;
                    if dropped_frames % 50 == 0 {
                        warn!(
                            "MF encoder worker: dropped {} frames due to queue backlog",
                            dropped_frames
                        );
                    }
                }

                let recv_at = Instant::now();
                let queue_wait_dur = recv_at.duration_since(job.enqueue_at);
                let encode_width = (job.width / 2) * 2;
                let encode_height = (job.height / 2) * 2;

                // エンコーダーの作成/再作成
                if encoder.is_none() || encode_width != width || encode_height != height {
                    if encoder.is_some() {
                        info!(
                            "MF encoder worker: resizing encoder {}x{} -> {}x{}",
                            width, height, encode_width, encode_height
                        );
                    }
                    width = encode_width;
                    height = encode_height;
                    match MfEncoder::new(width, height) {
                        Ok(enc) => encoder = Some(enc),
                        Err(e) => {
                            warn!("MF encoder worker: failed to create encoder: {}", e);
                            continue;
                        }
                    }
                    frame_timestamp = 0;
                }

                let encoder = encoder.as_mut().expect("encoder should be initialized");

                // RGBA→NV12変換
                let rgb_start = Instant::now();
                let nv12_data = rgba_to_nv12(
                    &job.rgba,
                    job.width,
                    job.height,
                    encode_width,
                    encode_height,
                );
                let rgb_dur = rgb_start.elapsed();

                // エンコード
                let encode_start = Instant::now();
                match encoder.encode(&nv12_data, frame_timestamp) {
                    Ok(encoded_data) => {
                        let encode_dur = encode_start.elapsed();

                        // Annex-B形式に変換
                        let pack_start = Instant::now();
                        let (sample_data, has_sps_pps) = annexb_from_mf_data(&encoded_data);
                        let pack_dur = pack_start.elapsed();

                        let sample_size = sample_data.len();
                        let total_dur = recv_at.elapsed() + queue_wait_dur;

                        if sample_size == 0 {
                            empty_samples += 1;
                            warn!(
                                "MF encoder worker: empty sample (total empty: {})",
                                empty_samples
                            );
                            continue;
                        }

                        successful_encodes += 1;
                        frame_timestamp += 1;

                        total_rgb_dur += rgb_dur;
                        total_encode_dur += encode_dur;
                        total_pack_dur += pack_dur;
                        total_queue_wait_dur += queue_wait_dur;

                        if successful_encodes % 50 == 0 || last_stats_log.elapsed().as_secs() >= 5 {
                            let avg_rgb = total_rgb_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_encode =
                                total_encode_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_queue =
                                total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
                            info!(
                                "MF encoder worker stats [{} frames]: avg_rgb={:.3}ms avg_encode={:.3}ms avg_pack={:.3}ms avg_queue={:.3}ms",
                                successful_encodes,
                                avg_rgb * 1000.0,
                                avg_encode * 1000.0,
                                avg_pack * 1000.0,
                                avg_queue * 1000.0
                            );
                            last_stats_log = Instant::now();
                        }

                        if res_tx
                            .send(EncodeResult {
                                sample_data,
                                is_keyframe: has_sps_pps,
                                duration: job.duration,
                                width: encode_width,
                                height: encode_height,
                                rgb_dur,
                                encode_dur,
                                pack_dur,
                                total_dur,
                                sample_size,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        encode_failures += 1;
                        warn!(
                            "MF encoder worker: encode failed: {} (total failures: {})",
                            e, encode_failures
                        );
                    }
                }
            }

            info!(
                "MF encoder worker: exiting (successful: {}, failures: {}, empty samples: {}, dropped frames: {})",
                successful_encodes, encode_failures, empty_samples, dropped_frames
            );
        });

        (vec![job_tx], res_rx)
    }
}
