#[cfg(windows)]
pub mod mf {
    use anyhow::{Context, Result};
    use std::mem::ManuallyDrop;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc as tokio_mpsc;
    use tracing::{info, warn};
    use windows::core::Array;
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFTransform, MFCreateMediaType, MFCreateSample, MFMediaType_Video, MFStartup,
        MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12, MFSTARTUP_FULL, MFT_ENUM_FLAG,
        MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO,
    };

    use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

    use crate::h264::rgba_to_yuv;

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
                crate::h264::openh264::start_encode_workers()
            }
        }

        fn codec(&self) -> VideoCodec {
            VideoCodec::H264
        }
    }

    /// Media Foundationが利用可能かチェック
    pub fn check_mf_available() -> bool {
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
    pub fn init_media_foundation() -> bool {
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

    fn enumerate_mfts(
        category: &windows::core::GUID,
        flags: MFT_ENUM_FLAG,
        input_type: Option<&MFT_REGISTER_TYPE_INFO>,
        output_type: Option<&MFT_REGISTER_TYPE_INFO>,
    ) -> Result<Vec<IMFActivate>> {
        let mut transform_sources = Vec::new();
        let mfactivate_list = unsafe {
            let mut data = std::ptr::null_mut();
            let mut len = 0;
            MFTEnumEx(
                *category,
                flags,
                input_type.map(|info| info as *const _),
                output_type.map(|info| info as *const _),
                &mut data,
                &mut len,
            )?;
            Array::<IMFActivate>::from_raw_parts(data as _, len)
        };
        if !mfactivate_list.is_empty() {
            for mfactivate in mfactivate_list.as_slice() {
                if let Some(transform_source) = mfactivate.clone() {
                    transform_sources.push(transform_source);
                }
            }
        }
        Ok(transform_sources)
    }

    /// H.264エンコーダーMFTを検索
    pub unsafe fn find_h264_encoder() -> Result<()> {
        let input_type = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };

        let output_type = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };

        let mfactivate_list = enumerate_mfts(
            &windows::core::GUID::zeroed(), // guidCategory
            MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SYNCMFT.0),
            Some(&input_type),
            Some(&output_type),
        )?;

        if mfactivate_list.is_empty() {
            return Err(anyhow::anyhow!("No H.264 encoder MFT found"));
        }

        // リソースをクリーンアップ
        for mfactivate in mfactivate_list.as_slice() {
            let _ = mfactivate.ShutdownObject();
        }

        Ok(())
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

            unsafe {
                let activate_array = enumerate_mfts(
                    &windows::core::GUID::zeroed(), // guidCategory
                    MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SYNCMFT.0),
                    Some(&input_type),
                    Some(&output_type),
                )?;

                // 最初のMFTを使用
                let activate = activate_array
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("No H.264 encoder MFT found"))?;

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
                for mfactivate in activate_array.as_slice() {
                    let _ = mfactivate.ShutdownObject();
                }
                // MEMO: activate_array で Vec にしてしまっているがもとのポインタは解放されてなくてメモリリークする可能性？
                // windows::Win32::System::Com::CoTaskMemFree(Some(activate_array as *mut _));

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
                const MAX_PROCESS_OUTPUT_ITERATIONS: usize = 100; // 無限ループ防止
                let mut iterations = 0;
                loop {
                    iterations += 1;
                    if iterations > MAX_PROCESS_OUTPUT_ITERATIONS {
                        return Err(anyhow::anyhow!(
                            "ProcessOutput loop exceeded maximum iterations ({})",
                            MAX_PROCESS_OUTPUT_ITERATIONS
                        ));
                    }

                    // 各イテレーションでoutput_data_bufferをリセット
                    output_data_buffer.pSample = ManuallyDrop::new(None);
                    output_data_buffer.pEvents = ManuallyDrop::new(None);

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
                            // pSampleがNoneの場合でも、次の呼び出しでMF_E_TRANSFORM_NEED_MORE_INPUTが返されるはず
                            // ループを継続して次の出力を待つ
                        }
                        Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                            // すべての出力を取得した - 正常終了
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
                            // エンコーダー作成失敗時は空の結果を送信してテストがタイムアウトしないようにする
                            if res_tx
                                .send(EncodeResult {
                                    sample_data: Vec::new(),
                                    is_keyframe: false,
                                    duration: job.duration,
                                    width: encode_width,
                                    height: encode_height,
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    }
                    frame_timestamp = 0;
                }

                let encoder = encoder.as_mut().expect("encoder should be initialized");

                // RGBA→NV12変換
                let rgb_start = Instant::now();
                let nv12_data = rgba_to_yuv::rgba_to_nv12(
                    &job.rgba,
                    encode_width as usize,
                    encode_height as usize,
                    job.width as usize,
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
                        // エンコードエラーが発生した場合でも、空の結果を送信してテストがタイムアウトしないようにする
                        // ただし、これは通常の動作ではないため、ログに記録する
                        if res_tx
                            .send(EncodeResult {
                                sample_data: Vec::new(),
                                is_keyframe: false,
                                duration: job.duration,
                                width: encode_width,
                                height: encode_height,
                            })
                            .is_err()
                        {
                            break;
                        }
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

#[cfg(test)]
#[path = "mmf_test.rs"]
mod mmf_test;
