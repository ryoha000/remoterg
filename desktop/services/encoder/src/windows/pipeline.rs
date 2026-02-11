use core_types::{EncodeJobSlot, EncodeResult, ShutdownError};
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, info, span, warn, Level};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    METransformHaveOutput, METransformNeedInput, MFCreateDXGISurfaceBuffer, MFCreateSample,
    MFSampleExtension_VideoEncodePictureType, MFT_OUTPUT_DATA_BUFFER, MF_EVENT_FLAG_NONE,
    MF_EVENT_TYPE, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
};

use crate::windows::codec::{CodecType, HardwareEncoder};
use crate::windows::utils::d3d::D3D11Resources;
use crate::windows::utils::media_type;
use crate::windows::utils::preprocessor::VideoProcessorPreprocessor;

/// 入力フレームのメタ情報（出力と対応付けるため）
struct InputFrameMeta {
    duration: Duration,
    width: u32,
    height: u32,
    frame_id: u64,
}

/// Media Foundationエンコードワーカーを起動
pub fn start_mf_encode_workers(
    codec_type: CodecType,
) -> (
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
        let mut input_meta_queue: VecDeque<InputFrameMeta> = VecDeque::new();

        // イベントループを開始する前に、エンコーダーが初期化されている必要がある
        // 最初のフレームが来るまで待機
        let first_job = match job_slot_clone.take() {
            Ok(job) => job,
            Err(ShutdownError) => {
                info!("MF encoder worker: received shutdown signal before initialization, exiting");
                return;
            }
        };

        // 最初のフレームで初期化
        let encode_width = (first_job.width / 2) * 2;
        let encode_height = (first_job.height / 2) * 2;

        let init_span = span!(Level::DEBUG, "mf_encoder_init");
        let _init_guard = init_span.enter();

        // D3D11 リソースの作成
        let d3d_resources = {
            let resource_span = span!(Level::DEBUG, "d3d_create_resources");
            let _resource_guard = resource_span.enter();
            match D3D11Resources::create() {
                Ok(resources) => resources,
                Err(e) => {
                    warn!("MF encoder worker: failed to create D3D11 resources: {}", e);
                    return;
                }
            }
        };

        let mut preprocessor = {
            let preproc_span = span!(Level::DEBUG, "preproc_create");
            let _preproc_guard = preproc_span.enter();
            match VideoProcessorPreprocessor::create(d3d_resources.clone()) {
                Ok(preproc) => preproc,
                Err(e) => {
                    warn!("MF encoder worker: failed to create preprocessor: {}", e);
                    return;
                }
            }
        };

        let mut encoder: Box<dyn HardwareEncoder> = {
            let encoder_span = span!(Level::DEBUG, "encoder_create");
            let _encoder_guard = encoder_span.enter();
            match codec_type.create_encoder(d3d_resources.clone(), encode_width, encode_height) {
                Ok(enc) => enc,
                Err(e) => {
                    warn!("MF encoder worker: failed to create encoder: {}", e);
                    return;
                }
            }
        };

        // ストリーミングを開始
        {
            let stream_span = span!(Level::DEBUG, "encoder_start_streaming");
            let _stream_guard = stream_span.enter();
            if let Err(e) = encoder.start_streaming() {
                warn!("MF encoder worker: failed to start streaming: {}", e);
                return;
            }
        }

        drop(_init_guard);

        // 最初のフレームを処理
        let mut pending_job = Some(first_job);

        // 参考実装に従い、常駐イベントループを開始
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
                                "MF encoder worker: failed to get event: {} (HRESULT: {:?})",
                                e,
                                e.code().0
                            );
                            encode_failures += 1;
                            // エラーが続く場合は終了
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
                        warn!("MF encoder worker: failed to get event type: {}", e);
                        continue;
                    }
                };

                match event_type {
                    #[allow(non_upper_case_globals)]
                    METransformNeedInput => {
                        // NeedInput イベントが来たときに最新のフレームを取得
                        // try_take()でノンブロッキング取得（最新の1つだけ）
                        let job = if let Some(job) = pending_job.take() {
                            job
                        } else {
                            // 最新のフレームを取得（利用可能な場合のみ）
                            match job_slot_clone.take() {
                                Ok(job) => job,
                                Err(ShutdownError) => {
                                    info!("MF encoder worker: received shutdown signal, exiting");
                                    break;
                                }
                            }
                        };

                        let job_width = job.width;
                        let job_height = job.height;

                        // 詳細な処理内訳を計測するスパンを開始
                        let handle_input_span =
                            span!(Level::DEBUG, "handle_need_input", frame_id = job.frame_id);
                        let _handle_input_guard = handle_input_span.enter();

                        // エンコード解像度は2の倍数に調整
                        let encode_width = (job_width / 2) * 2;
                        let encode_height = (job_height / 2) * 2;

                        // エンコーダーの解像度を更新
                        if let Err(e) = {
                            use windows::Win32::Media::MediaFoundation::{
                                MFVideoFormat_AV1, MFVideoFormat_H264,
                            };
                            media_type::setup_media_types(
                                encoder.transform(),
                                encode_width,
                                encode_height,
                                match codec_type {
                                    CodecType::H264 => &MFVideoFormat_H264,
                                    CodecType::AV1 => &MFVideoFormat_AV1,
                                },
                            )
                        } {
                            warn!(
                                "MF encoder worker: failed to setup media types for resize: {}",
                                e
                            );
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        // 前処理（RGBA → NV12 テクスチャ）
                        // src: job_width/height, dst: encode_width/height
                        let nv12_texture = {
                            let preprocess_span =
                                span!(Level::DEBUG, "preprocess", frame_id = job.frame_id);
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
                                            "MF encoder worker: preprocess failed for {}x{} frame: {} (HRESULT: {:?})",
                                            job.width, job.height, e, e.source()
                                        );
                                    encode_failures += 1;
                                    input_meta_queue.pop_back(); // メタ情報も削除
                                    continue;
                                }
                            }
                        };

                        // タイムスタンプから duration を計算
                        // windows_timespan は100ナノ秒単位の SystemRelativeTime（単調増加）
                        let duration = if let Some(prev_ts) = last_timestamp {
                            let delta_hns = job.timestamp.saturating_sub(prev_ts).max(1);
                            // 100ナノ秒単位からナノ秒単位に変換
                            // u64 の最大値は約584年分の100ナノ秒なので、オーバーフローを防ぐためにチェック
                            let delta_ns = delta_hns.saturating_mul(100);
                            Duration::from_nanos(delta_ns)
                        } else {
                            // 最初のフレーム: 1/60s = 約16.67ms
                            Duration::from_millis(16)
                        };
                        last_timestamp = Some(job.timestamp);

                        // メタ情報をキューに保存
                        input_meta_queue.push_back(InputFrameMeta {
                            duration,
                            width: encode_width,
                            height: encode_height,
                            frame_id: job.frame_id,
                        });

                        // DXGI サーフェスバッファを作成
                        let input_buffer = {
                            let buffer_create_span =
                                span!(Level::DEBUG, "buffer_create", frame_id = job.frame_id);
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
                                        "MF encoder worker: failed to create DXGI surface buffer: {}",
                                        e
                                    );
                                    encode_failures += 1;
                                    input_meta_queue.pop_back(); // メタ情報も削除
                                    continue;
                                }
                            }
                        };

                        // 入力サンプルを作成
                        let input_sample = match MFCreateSample() {
                            Ok(sample) => sample,
                            Err(e) => {
                                warn!("MF encoder worker: failed to create input sample: {}", e);
                                encode_failures += 1;
                                input_meta_queue.pop_back();
                                continue;
                            }
                        };

                        if let Err(e) = input_sample.AddBuffer(&input_buffer) {
                            warn!("MF encoder worker: failed to add buffer to sample: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        // サンプルタイムと継続時間を設定
                        // duration を 100ns 単位に変換
                        let sample_time_hns = frame_timestamp;
                        let sample_duration_hns = duration.as_nanos() as i64 / 100;

                        if let Err(e) = input_sample.SetSampleTime(sample_time_hns) {
                            warn!("MF encoder worker: failed to set sample time: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        let _ = input_sample.SetSampleDuration(sample_duration_hns);

                        // キーフレーム要求がある場合は強制
                        if job.request_keyframe {
                            if let Err(e) =
                                input_sample.SetUINT32(&MFSampleExtension_VideoEncodePictureType, 1)
                            {
                                warn!("MF encoder worker: failed to set picture type: {}", e);
                                encode_failures += 1;
                                input_meta_queue.pop_back();
                                continue;
                            }
                        }

                        // ProcessInput を呼び出す
                        {
                            let process_input_span =
                                span!(Level::DEBUG, "process_input", frame_id = job.frame_id);
                            let _guard = process_input_span.enter();

                            if let Err(e) = encoder.transform().ProcessInput(0, &input_sample, 0) {
                                warn!(
                                    "MF encoder worker: ProcessInput failed for {}x{} frame: {} (HRESULT: {:?})",
                                    job_width, job_height, e, e.code().0
                                );
                                encode_failures += 1;
                                input_meta_queue.pop_back();
                                // エラーが続く場合は警告を出力
                                if encode_failures > 5 {
                                    warn!(
                                        "MF encoder worker: ProcessInput failures exceeded threshold ({} failures)",
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
                        // 出力が準備できた場合、ProcessOutputを呼んでデータを取得
                        let output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                            dwStreamID: 0,
                            pSample: ManuallyDrop::new(None),
                            dwStatus: 0,
                            pEvents: ManuallyDrop::new(None),
                        };
                        let mut status: u32 = 0;

                        let mut output_buffers = [output_data_buffer];

                        let process_output_result = {
                            let expected_frame_id =
                                input_meta_queue.front().map(|m| m.frame_id).unwrap_or(0);
                            let process_output_span =
                                span!(Level::DEBUG, "process_output", frame_id = expected_frame_id);
                            let _guard = process_output_span.enter();

                            encoder
                                .transform()
                                .ProcessOutput(0, &mut output_buffers, &mut status)
                        };

                        match process_output_result {
                            Ok(_) => {
                                if let Some(sample) = output_buffers[0].pSample.take() {
                                    // HardwareEncoderトレイト経由でデータを取得
                                    match encoder.process_output(&sample) {
                                        Ok(encoded_frame) => {
                                            // メタ情報を取得
                                            let meta = match input_meta_queue.pop_front() {
                                                Some(m) => m,
                                                None => {
                                                    warn!("MF encoder worker: no input meta available for output");
                                                    empty_samples += 1;
                                                    continue;
                                                }
                                            };

                                            if encoded_frame.data.is_empty() {
                                                empty_samples += 1;
                                                warn!(
                                                    "MF encoder worker: empty sample (total empty: {})",
                                                    empty_samples
                                                );
                                                continue;
                                            }

                                            if res_tx
                                                .send(EncodeResult {
                                                    sample_data: encoded_frame.data,
                                                    is_keyframe: encoded_frame.is_keyframe,
                                                    duration: meta.duration,
                                                    width: meta.width,
                                                    height: meta.height,
                                                    frame_id: meta.frame_id,
                                                })
                                                .is_err()
                                            {
                                                // 受信側が閉じられた
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                "MF encoder worker: process_output failed: {}",
                                                e
                                            );
                                            encode_failures += 1;
                                            // メタ情報の整合性を保つため、エラー時もポップするか検討が必要だが、
                                            // ここでは次のProcessOutputが成功する可能性も考慮し、
                                            // 対応付けがずれるリスクはあるがポップしないでおくか、
                                            // process_outputが失敗した＝データが取れなかった＝メタも消費すべきか。
                                            // ここでは消費しておく。
                                            input_meta_queue.pop_front();
                                            continue;
                                        }
                                    }
                                } else {
                                    empty_samples += 1;
                                    warn!(
                                        "MF encoder worker: ProcessOutput returned empty sample (total empty: {})",
                                        empty_samples
                                    );
                                }
                            }
                            Err(e) if e.code().0 == MF_E_TRANSFORM_NEED_MORE_INPUT.0 => {
                                // すべての出力を取得した - 正常（次のNeedInputを待つ）
                                debug!("MF encoder worker: all output retrieved");
                            }
                            Err(e) if e.code().0 == MF_E_TRANSFORM_STREAM_CHANGE.0 => {
                                warn!("MF encoder worker: stream change detected");
                                // ストリーム変更が発生した場合は再初期化が必要かもしれないが、
                                // ここでは警告のみ
                            }
                            Err(e) => {
                                let error_code = e.code();
                                warn!(
                                    "MF encoder worker: ProcessOutput failed: {} (code: {:?}, status: {})",
                                    e,
                                    error_code.0,
                                    status
                                );
                                encode_failures += 1;
                                // エラーが続く場合は警告を出力
                                if encode_failures > 5 {
                                    warn!(
                                        "MF encoder worker: ProcessOutput failures exceeded threshold ({} failures)",
                                        encode_failures
                                    );
                                }
                                // MF_E_TRANSFORM_NEED_MORE_INPUT の場合は次の入力待ちに続行
                                if error_code.0 == MF_E_TRANSFORM_NEED_MORE_INPUT.0 {
                                    debug!("MF encoder worker: need more input, continuing");
                                }
                            }
                        }
                    }
                    _ => {
                        // その他のイベントは無視して続行
                        debug!("MF encoder worker: ignoring event type: {:?}", event_type);
                    }
                }
            }
        }

        info!(
            "MF encoder worker: exiting (failures: {}, empty samples: {})",
            encode_failures, empty_samples
        );
    });

    (job_slot, res_rx)
}
