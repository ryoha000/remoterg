use core_types::{EncodeJobSlot, EncodeResult, ShutdownError};
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, info, warn, span, Level};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    METransformHaveOutput, METransformNeedInput, MFCreateDXGISurfaceBuffer, MFCreateSample,
    MFSampleExtension_CleanPoint, MFSampleExtension_VideoEncodePictureType, MFT_OUTPUT_DATA_BUFFER,
    MF_EVENT_FLAG_NONE, MF_EVENT_TYPE, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE,
};

// Reuse existing D3D and Preprocessor from Utils
use crate::windows::utils::d3d::D3D11Resources;
use crate::windows::utils::preprocessor::VideoProcessorPreprocessor;
use crate::windows::av1::encoder::AV1Encoder;

/// 入力フレームのメタ情報（出力と対応付けるため）
struct InputFrameMeta {
    duration: Duration,
    width: u32,
    height: u32,
    frame_id: u64,
}

/// Media Foundation AV1 エンコードワーカーを起動
pub fn start_mf_encode_workers() -> (
    Arc<EncodeJobSlot>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let job_slot = EncodeJobSlot::new();
    let job_slot_clone = Arc::clone(&job_slot);
    let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

    std::thread::spawn(move || {
        let mut encode_failures = 0u32;
        let mut empty_samples = 0u32;
        let mut frame_timestamp = 0i64;
        let mut last_timestamp: Option<u64> = None;

        // 入力/出力の対応付け用キュー
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

        let mut input_meta_queue: VecDeque<InputFrameMeta> = VecDeque::new();

        unsafe {
            if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
                warn!("MF AV1 encoder worker: failed to initialize COM: {}", e);
            }
        }

        // イベントループを開始する前に、エンコーダーが初期化されている必要がある
        // 最初のフレームが来るまで待機
        let first_job = match job_slot_clone.take() {
            Ok(job) => job,
            Err(ShutdownError) => {
                info!("MF AV1 encoder worker: received shutdown signal before initialization, exiting");
                return;
            }
        };

        // 最初のフレームで初期化
        // 最初のフレームで初期化
        // 一部のハードウェアエンコーダーは16の倍数の解像度を要求するため、16の倍数にアライメント（切り捨て）
        let encode_width = (first_job.width / 16 * 16).max(16);
        let encode_height = (first_job.height / 16 * 16).max(16);

        let init_span = span!(Level::DEBUG, "mf_av1_encoder_init");
        let _init_guard = init_span.enter();

        // D3D11 リソースの作成
        let d3d_resources = {
            let resource_span = span!(Level::DEBUG, "d3d_create_resources");
            let _resource_guard = resource_span.enter();
            match D3D11Resources::create() {
                Ok(resources) => resources,
                Err(e) => {
                    warn!("MF AV1 encoder worker: failed to create D3D11 resources: {}", e);
                    return;
                }
            }
        };

        let mut preprocessor = {
            let preproc_span = span!(Level::DEBUG, "preproc_create");
            let _preproc_guard = preproc_span.enter();
            match VideoProcessorPreprocessor::create(
                d3d_resources.clone(),
            ) {
                Ok(preproc) => preproc,
                Err(e) => {
                    warn!("MF AV1 encoder worker: failed to create preprocessor: {}", e);
                    return;
                }
            }
        };

        let mut encoder = {
            let encoder_span = span!(Level::DEBUG, "encoder_create");
            let _encoder_guard = encoder_span.enter();
            match AV1Encoder::create(d3d_resources.clone(), encode_width, encode_height)
            {
                Ok(enc) => enc,
                Err(e) => {
                    warn!("MF AV1 encoder worker: failed to create encoder: {:?}", e);
                    return;
                }
            }
        };

        // codec configからSPS/PPS (Sequence Header)を取得
        let codec_config_blob = encoder.get_codec_config();
        if codec_config_blob.is_some() {
            info!("MF AV1 encoder worker: extracted codec config");
        } else {
            debug!("MF AV1 encoder worker: codec config not available");
        }

        // ストリーミングを開始
        {
            let stream_span = span!(Level::DEBUG, "encoder_start_streaming");
            let _stream_guard = stream_span.enter();
            if let Err(e) = encoder.start_streaming() {
                warn!("MF AV1 encoder worker: failed to start streaming: {}", e);
                return;
            }
        }
        
        drop(_init_guard);

        // 最初のフレームを処理
        let mut pending_job = Some(first_job);
        let mut first_keyframe_sent = false;

        // 常駐イベントループを開始
        loop {
            unsafe {
                // イベントを待機
                let event = {
                    let wait_span = span!(Level::TRACE, "wait_for_event");
                    let _guard = wait_span.enter();
                    match encoder.event_generator().GetEvent(MF_EVENT_FLAG_NONE) {
                        Ok(event) => event,
                        Err(e) => {
                            warn!(
                                "MF AV1 encoder worker: failed to get event: {} (HRESULT: {:?})",
                                e,
                                e.code()
                            );
                            encode_failures += 1;
                            if encode_failures > 10 {
                                break;
                            }
                            continue;
                        }
                    }
                };

                let event_type = match event.GetType() {
                    Ok(ty) => MF_EVENT_TYPE(ty as i32),
                    Err(e) => {
                        warn!("MF AV1 encoder worker: failed to get event type: {}", e);
                        continue;
                    }
                };

                match event_type {
                    #[allow(non_upper_case_globals)]
                    METransformNeedInput => {
                        let job = if let Some(job) = pending_job.take() {
                            job
                        } else {
                            match job_slot_clone.take() {
                                Ok(job) => job,
                                Err(ShutdownError) => {
                                    info!("MF AV1 encoder worker: received shutdown signal, exiting");
                                    break;
                                }
                            }
                        };

                        let job_width = job.width;
                        let job_height = job.height;

                        let handle_input_span = span!(
                            Level::DEBUG,
                            "handle_need_input",
                            frame_id = job.frame_id
                        );
                        let _handle_input_guard = handle_input_span.enter();
                        
                        // エンコード解像度は16の倍数に調整（多くのハードウェアエンコーダーの要件）
                        // 切り捨てて16の倍数にする
                        let encode_width = (job_width / 16 * 16).max(16);
                        let encode_height = (job_height / 16 * 16).max(16);

                        if let Err(e) = encoder.resize(encode_width, encode_height) {
                            warn!("MF AV1 encoder worker: failed to resize encoder: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        let nv12_texture = {
                            let preprocess_span = span!(
                                Level::DEBUG,
                                "preprocess",
                                frame_id = job.frame_id
                            );
                            let _guard = preprocess_span.enter();

                            match preprocessor.process(
                                &job.rgba,
                                job_width,
                                job_height,
                                encode_width,
                                encode_height,
                                frame_timestamp,
                            ) {
                                Ok(texture) => texture,
                                Err(e) => {
                                    warn!(
                                            "MF AV1 encoder worker: preprocess failed for {}x{} frame: {} (HRESULT: {:?})",
                                            job.width, job.height, e, e.source()
                                        );
                                    encode_failures += 1;
                                    input_meta_queue.pop_back();
                                    continue;
                                }
                            }
                        };

                        let duration = if let Some(prev_ts) = last_timestamp {
                            let delta_hns = job.timestamp.saturating_sub(prev_ts).max(1);
                            let delta_ns = delta_hns.saturating_mul(100);
                            Duration::from_nanos(delta_ns)
                        } else {
                            Duration::from_millis(16)
                        };
                        last_timestamp = Some(job.timestamp);

                        input_meta_queue.push_back(InputFrameMeta {
                            duration,
                            width: encode_width,
                            height: encode_height,
                            frame_id: job.frame_id,
                        });

                        let input_buffer = {
                             let buffer_create_span = span!(
                                Level::DEBUG,
                                "buffer_create",
                                frame_id = job.frame_id
                            );
                            let _guard = buffer_create_span.enter();
                            
                            match MFCreateDXGISurfaceBuffer(
                                &ID3D11Texture2D::IID,
                                &nv12_texture,
                                0,
                                false,
                            ) {
                                Ok(buffer) => buffer,
                                Err(e) => {
                                    warn!(
                                        "MF AV1 encoder worker: failed to create DXGI surface buffer: {}",
                                        e
                                    );
                                    encode_failures += 1;
                                    input_meta_queue.pop_back();
                                    continue;
                                }
                            }
                        };

                        let input_sample = match MFCreateSample() {
                            Ok(sample) => sample,
                            Err(e) => {
                                warn!("MF AV1 encoder worker: failed to create input sample: {}", e);
                                encode_failures += 1;
                                input_meta_queue.pop_back();
                                continue;
                            }
                        };

                        if let Err(e) = input_sample.AddBuffer(&input_buffer) {
                            warn!("MF AV1 encoder worker: failed to add buffer to sample: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        let sample_time_hns = frame_timestamp;
                        let sample_duration_hns = duration.as_nanos() as i64 / 100;

                        if let Err(e) = input_sample.SetSampleTime(sample_time_hns) {
                            warn!("MF AV1 encoder worker: failed to set sample time: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        let _ = input_sample.SetSampleDuration(sample_duration_hns);

                        if job.request_keyframe {
                            if let Err(e) =
                                input_sample.SetUINT32(&MFSampleExtension_VideoEncodePictureType, 1)
                            {
                                warn!("MF AV1 encoder worker: failed to set picture type: {}", e);
                                encode_failures += 1;
                                input_meta_queue.pop_back();
                                continue;
                            }
                        }

                        {
                            let process_input_span = span!(
                                Level::DEBUG,
                                "process_input",
                                frame_id = job.frame_id
                            );
                            let _guard = process_input_span.enter();
                            
                            if let Err(e) = encoder.transform().ProcessInput(0, &input_sample, 0) {
                                warn!(
                                    "MF AV1 encoder worker: ProcessInput failed for {}x{} frame (encoded as {}x{}): {} (HRESULT: {:?})",
                                    job_width, job_height, encode_width, encode_height, e, e.code()
                                );
                                encode_failures += 1;
                                input_meta_queue.pop_back();
                                if encode_failures > 5 {
                                    warn!(
                                        "MF AV1 encoder worker: ProcessInput failures exceeded threshold ({} failures)",
                                        encode_failures
                                    );
                                }
                                continue;
                            }
                        }

                        frame_timestamp += sample_duration_hns;
                    }
                    #[allow(non_upper_case_globals)]
                    METransformHaveOutput => {
                        let output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                            dwStreamID: 0,
                            pSample: ManuallyDrop::new(None),
                            dwStatus: 0,
                            pEvents: ManuallyDrop::new(None),
                        };
                        let mut status: u32 = 0;
                        let mut output_buffers = [output_data_buffer];
                        
                        let process_output_result = {
                            let expected_frame_id = input_meta_queue.front().map(|m| m.frame_id).unwrap_or(0);
                            let process_output_span = span!(
                                Level::DEBUG,
                                "process_output",
                                frame_id = expected_frame_id
                            );
                            let _guard = process_output_span.enter();
                            
                            encoder
                                .transform()
                                .ProcessOutput(0, &mut output_buffers, &mut status)
                        };

                        match process_output_result
                        {
                            Ok(_) => {
                                if let Some(sample) = output_buffers[0].pSample.take() {
                                    let buffer = match sample.GetBufferByIndex(0) {
                                        Ok(buf) => buf,
                                        Err(e) => {
                                            warn!("MF AV1 encoder worker: failed to get output buffer: {}", e);
                                            empty_samples += 1;
                                            continue;
                                        }
                                    };

                                    let mut data_ptr: *mut u8 = std::ptr::null_mut();
                                    let mut max_length: u32 = 0;
                                    if let Err(e) =
                                        buffer.Lock(&mut data_ptr, Some(&mut max_length), None)
                                    {
                                        warn!("MF AV1 encoder worker: failed to lock output buffer: {}", e);
                                        empty_samples += 1;
                                        continue;
                                    }

                                    let current_length = match buffer.GetCurrentLength() {
                                        Ok(len) => len,
                                        Err(e) => {
                                            warn!("MF AV1 encoder worker: failed to get output buffer length: {}", e);
                                            let _ = buffer.Unlock();
                                            empty_samples += 1;
                                            continue;
                                        }
                                    };

                                    let mut encoded_data = Vec::new();
                                    if current_length > 0 && !data_ptr.is_null() {
                                        let slice = std::slice::from_raw_parts(
                                            data_ptr,
                                            current_length as usize,
                                        );
                                        encoded_data.extend_from_slice(slice);
                                    }

                                    if let Err(e) = buffer.Unlock() {
                                        warn!("MF AV1 encoder worker: failed to unlock output buffer: {}", e);
                                    }

                                    // キーフレーム判定（MFSampleExtension_CleanPoint）
                                    let is_clean_point =
                                        match sample.GetUINT32(&MFSampleExtension_CleanPoint) {
                                            Ok(1) => true,
                                            Ok(0) => false,
                                            _ => false,
                                        };
                                    let is_keyframe = is_clean_point;

                                    // もし最初のキーフレームなら、codec_config (Sequence Header) があれば注入する
                                    // 通常AV1のキーフレームOBUはSequence Headerを含まないかもしれない
                                    // WebRTCではキーフレームの前にSequence Headerが必要
                                    if is_keyframe && !first_keyframe_sent {
                                        if let Some(ref config) = codec_config_blob {
                                            debug!("MF AV1 encoder: injecting codec config to first keyframe ({} bytes)", config.len());
                                            let mut injected_data = Vec::with_capacity(config.len() + encoded_data.len());
                                            injected_data.extend_from_slice(config);
                                            injected_data.extend_from_slice(&encoded_data);
                                            encoded_data = injected_data;
                                        }
                                        first_keyframe_sent = true;
                                    }
                                    
                                    // メタ情報を取得
                                    let meta = match input_meta_queue.pop_front() {
                                        Some(m) => m,
                                        None => {
                                            warn!("MF AV1 encoder worker: no input meta available for output");
                                            empty_samples += 1;
                                            continue;
                                        }
                                    };

                                    if encoded_data.is_empty() {
                                        empty_samples += 1;
                                        warn!("MF AV1 encoder worker: empty sample (total empty: {})", empty_samples);
                                        continue;
                                    }

                                    if res_tx
                                        .send(EncodeResult {
                                            sample_data: encoded_data,
                                            is_keyframe: is_keyframe,
                                            duration: meta.duration,
                                            width: meta.width,
                                            height: meta.height,
                                            frame_id: meta.frame_id,
                                        })
                                        .is_err()
                                    {
                                        break;
                                    }
                                } else {
                                    empty_samples += 1;
                                    warn!("MF AV1 encoder worker: ProcessOutput returned empty sample");
                                }
                            }
                            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                                debug!("MF AV1 encoder worker: all output retrieved");
                            }
                            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                                warn!("MF AV1 encoder worker: stream change detected");
                            }
                            Err(e) => {
                                let error_code = e.code();
                                warn!(
                                    "MF AV1 encoder worker: ProcessOutput failed: {} (code: {:?}, status: {})",
                                    e,
                                    error_code,
                                    status
                                );
                                encode_failures += 1;
                                if encode_failures > 5 {
                                    warn!("MF AV1 encoder worker: ProcessOutput failures exceeded threshold");
                                }
                                if error_code == MF_E_TRANSFORM_NEED_MORE_INPUT {
                                    debug!("MF AV1 encoder worker: need more input, continuing");
                                }
                            }
                        }
                    }
                    _ => {
                        debug!("MF AV1 encoder worker: ignoring event type: {:?}", event_type);
                    }
                }
            }
        }

        info!("MF AV1 encoder worker: exiting");
    });

    (job_slot, res_rx)
}
