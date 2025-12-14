use core_types::{EncodeJob, EncodeResult};
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, info, warn};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    METransformHaveOutput, METransformNeedInput, MFCreateDXGISurfaceBuffer, MFCreateSample,
    MFT_OUTPUT_DATA_BUFFER, MF_EVENT_FLAG_NONE, MF_EVENT_TYPE, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE,
};

use crate::h264::mmf::d3d::D3D11Resources;
use crate::h264::mmf::encoder::H264Encoder;
use crate::h264::mmf::preprocessor::VideoProcessorPreprocessor;

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

/// RGBA を BGRA に変換（簡易実装）
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = Vec::with_capacity(rgba.len());
    for chunk in rgba.chunks_exact(4) {
        bgra.push(chunk[2]); // B
        bgra.push(chunk[1]); // G
        bgra.push(chunk[0]); // R
        bgra.push(chunk[3]); // A
    }
    bgra
}

/// 入力フレームのメタ情報（出力と対応付けるため）
struct InputFrameMeta {
    duration: Duration,
    width: u32,
    height: u32,
}

/// Media Foundationエンコードワーカーを起動
pub fn start_mf_encode_workers() -> (
    Vec<std::sync::mpsc::Sender<EncodeJob>>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let (job_tx, job_rx) = std::sync::mpsc::channel::<EncodeJob>();
    let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

    std::thread::spawn(move || {
        let mut encode_failures = 0u32;
        let mut empty_samples = 0u32;
        let mut successful_encodes = 0u32;
        let mut frame_timestamp = 0i64;

        // パフォーマンス統計用
        let mut total_preprocess_dur = Duration::ZERO;
        let mut total_encode_dur = Duration::ZERO;
        let mut total_pack_dur = Duration::ZERO;
        let mut total_queue_wait_dur = Duration::ZERO;
        let mut last_stats_log = Instant::now();

        // 入力/出力の対応付け用キュー
        let mut input_meta_queue: VecDeque<InputFrameMeta> = VecDeque::new();

        // イベントループを開始する前に、エンコーダーが初期化されている必要がある
        // 最初のフレームが来るまで待機
        let first_job = match job_rx.recv() {
            Ok(job) => job,
            Err(_) => {
                info!("MF encoder worker: no jobs received, exiting");
                return;
            }
        };

        // 最初のフレームで初期化
        let encode_width = (first_job.width / 2) * 2;
        let encode_height = (first_job.height / 2) * 2;

        // D3D11 リソースの作成
        let d3d_resources = match D3D11Resources::create() {
            Ok(resources) => resources,
            Err(e) => {
                warn!("MF encoder worker: failed to create D3D11 resources: {}", e);
                return;
            }
        };

        // エンコーダーと前処理器の作成
        let mut width = encode_width;
        let mut height = encode_height;

        let mut preprocessor = match VideoProcessorPreprocessor::create(
            d3d_resources.clone(),
            encode_width,
            encode_height,
        ) {
            Ok(preproc) => preproc,
            Err(e) => {
                warn!("MF encoder worker: failed to create preprocessor: {}", e);
                return;
            }
        };

        let mut encoder =
            match H264Encoder::create(d3d_resources.clone(), encode_width, encode_height) {
                Ok(enc) => enc,
                Err(e) => {
                    warn!("MF encoder worker: failed to create encoder: {}", e);
                    return;
                }
            };

        // ストリーミングを開始
        if let Err(e) = encoder.start_streaming() {
            warn!("MF encoder worker: failed to start streaming: {}", e);
            return;
        }

        // 最初のフレームを処理
        let mut pending_job = Some(first_job);

        // 参考実装に従い、常駐イベントループを開始
        loop {
            unsafe {
                // イベントを待機
                let event = match encoder.event_generator().GetEvent(MF_EVENT_FLAG_NONE) {
                    Ok(event) => event,
                    Err(e) => {
                        warn!(
                            "MF encoder worker: failed to get event: {} (HRESULT: {:?})",
                            e,
                            e.code()
                        );
                        encode_failures += 1;
                        // エラーが続く場合は終了
                        if encode_failures > 10 {
                            break;
                        }
                        continue;
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
                        // 参考実装に従い、NeedInput イベントが来たときにフレームを取得
                        // 参考実装では blocking_recv() を使用しているが、テストの有限入力に対応するため
                        // try_recv() を使用してブロックを避ける
                        let job = if let Some(job) = pending_job.take() {
                            job
                        } else {
                            // キューからFIFOで1件取得（ドロップしない）
                            match job_rx.try_recv() {
                                Ok(job) => job,
                                Err(std::sync::mpsc::TryRecvError::Empty) => {
                                    // 入力が無い場合はそのままループ継続（HaveOutputイベントを処理できるようにする）
                                    continue;
                                }
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                    // チャンネルが閉じられた
                                    debug!("MF encoder worker: job channel closed");
                                    break;
                                }
                            }
                        };

                        let recv_at = Instant::now();
                        let queue_wait_dur = recv_at.duration_since(job.enqueue_at);
                        let job_width = (job.width / 2) * 2;
                        let job_height = (job.height / 2) * 2;

                        // 解像度が変更された場合は再初期化
                        if job_width != width || job_height != height {
                            info!(
                                "MF encoder worker: resizing encoder {}x{} -> {}x{}",
                                width, height, job_width, job_height
                            );
                            width = job_width;
                            height = job_height;

                            preprocessor = match VideoProcessorPreprocessor::create(
                                d3d_resources.clone(),
                                width,
                                height,
                            ) {
                                Ok(preproc) => preproc,
                                Err(e) => {
                                    warn!(
                                        "MF encoder worker: failed to recreate preprocessor: {}",
                                        e
                                    );
                                    encode_failures += 1;
                                    continue;
                                }
                            };

                            encoder =
                                match H264Encoder::create(d3d_resources.clone(), width, height) {
                                    Ok(enc) => enc,
                                    Err(e) => {
                                        warn!(
                                            "MF encoder worker: failed to recreate encoder: {}",
                                            e
                                        );
                                        encode_failures += 1;
                                        continue;
                                    }
                                };

                            if let Err(e) = encoder.start_streaming() {
                                warn!("MF encoder worker: failed to restart streaming: {}", e);
                                encode_failures += 1;
                                continue;
                            }

                            frame_timestamp = 0;
                            input_meta_queue.clear();

                            // エンコーダー再初期化後は、次のNeedInputイベントが来るまで待つ必要がある
                            // このフレームをpending_jobに保存して、次のNeedInputイベントで処理する
                            pending_job = Some(job);
                            continue;
                        }

                        // RGBA → BGRA 変換
                        let bgra_data = rgba_to_bgra(&job.rgba);

                        // 前処理（BGRA → NV12 テクスチャ）
                        let preprocess_start = Instant::now();
                        let nv12_texture = match preprocessor.process(
                            &bgra_data,
                            width,
                            height,
                            frame_timestamp,
                        ) {
                            Ok(texture) => texture,
                            Err(e) => {
                                warn!("MF encoder worker: preprocess failed: {}", e);
                                encode_failures += 1;
                                continue;
                            }
                        };
                        let preprocess_dur = preprocess_start.elapsed();

                        // メタ情報をキューに保存
                        input_meta_queue.push_back(InputFrameMeta {
                            duration: job.duration,
                            width: job_width,
                            height: job_height,
                        });

                        // DXGI サーフェスバッファを作成
                        let input_buffer = match MFCreateDXGISurfaceBuffer(
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
                        // 参考実装では 10_000_000 / framerate を使用しているが、
                        // ここでは job.duration を 100ns 単位に変換
                        let sample_time_hns = frame_timestamp;
                        let sample_duration_hns = job.duration.as_nanos() as i64 / 100;

                        if let Err(e) = input_sample.SetSampleTime(sample_time_hns) {
                            warn!("MF encoder worker: failed to set sample time: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        let _ = input_sample.SetSampleDuration(sample_duration_hns);

                        // ProcessInput を呼び出す
                        if let Err(e) = encoder.transform().ProcessInput(0, &input_sample, 0) {
                            warn!("MF encoder worker: ProcessInput failed: {}", e);
                            encode_failures += 1;
                            input_meta_queue.pop_back();
                            continue;
                        }

                        frame_timestamp += sample_duration_hns;
                        total_preprocess_dur += preprocess_dur;
                        total_queue_wait_dur += queue_wait_dur;
                    }
                    #[allow(non_upper_case_globals)]
                    METransformHaveOutput => {
                        // 出力が準備できた場合、ProcessOutputを呼んでデータを取得
                        let encode_start = Instant::now();
                        let output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                            dwStreamID: 0,
                            pSample: ManuallyDrop::new(None),
                            dwStatus: 0,
                            pEvents: ManuallyDrop::new(None),
                        };
                        let mut status: u32 = 0;

                        let mut output_buffers = [output_data_buffer];
                        match encoder
                            .transform()
                            .ProcessOutput(0, &mut output_buffers, &mut status)
                        {
                            Ok(_) => {
                                if let Some(sample) = output_buffers[0].pSample.take() {
                                    let buffer = match sample.GetBufferByIndex(0) {
                                        Ok(buf) => buf,
                                        Err(e) => {
                                            warn!(
                                                "MF encoder worker: failed to get output buffer: {}",
                                                e
                                            );
                                            empty_samples += 1;
                                            continue;
                                        }
                                    };

                                    let mut data_ptr: *mut u8 = std::ptr::null_mut();
                                    let mut max_length: u32 = 0;
                                    if let Err(e) =
                                        buffer.Lock(&mut data_ptr, Some(&mut max_length), None)
                                    {
                                        warn!(
                                            "MF encoder worker: failed to lock output buffer: {}",
                                            e
                                        );
                                        empty_samples += 1;
                                        continue;
                                    }

                                    let current_length = match buffer.GetCurrentLength() {
                                        Ok(len) => len,
                                        Err(e) => {
                                            warn!(
                                                "MF encoder worker: failed to get output buffer length: {}",
                                                e
                                            );
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
                                        warn!(
                                            "MF encoder worker: failed to unlock output buffer: {}",
                                            e
                                        );
                                    }

                                    let encode_dur = encode_start.elapsed();

                                    // Annex-B形式に変換
                                    let pack_start = Instant::now();
                                    let (sample_data, has_sps_pps) =
                                        annexb_from_mf_data(&encoded_data);
                                    let pack_dur = pack_start.elapsed();

                                    // メタ情報を取得
                                    let meta = match input_meta_queue.pop_front() {
                                        Some(m) => m,
                                        None => {
                                            warn!("MF encoder worker: no input meta available for output");
                                            empty_samples += 1;
                                            continue;
                                        }
                                    };

                                    if sample_data.is_empty() {
                                        empty_samples += 1;
                                        warn!(
                                            "MF encoder worker: empty sample (total empty: {})",
                                            empty_samples
                                        );
                                        continue;
                                    }

                                    successful_encodes += 1;
                                    total_encode_dur += encode_dur;
                                    total_pack_dur += pack_dur;

                                    if successful_encodes % 50 == 0
                                        || last_stats_log.elapsed().as_secs() >= 5
                                    {
                                        let avg_preprocess = total_preprocess_dur.as_secs_f64()
                                            / successful_encodes as f64;
                                        let avg_encode = total_encode_dur.as_secs_f64()
                                            / successful_encodes as f64;
                                        let avg_pack = total_pack_dur.as_secs_f64()
                                            / successful_encodes as f64;
                                        let avg_queue = total_queue_wait_dur.as_secs_f64()
                                            / successful_encodes as f64;
                                        info!(
                                            "MF encoder worker stats [{} frames]: avg_preprocess={:.3}ms avg_encode={:.3}ms avg_pack={:.3}ms avg_queue={:.3}ms",
                                            successful_encodes,
                                            avg_preprocess * 1000.0,
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
                                            duration: meta.duration,
                                            width: meta.width,
                                            height: meta.height,
                                        })
                                        .is_err()
                                    {
                                        // 受信側が閉じられた
                                        break;
                                    }
                                } else {
                                    empty_samples += 1;
                                    warn!(
                                        "MF encoder worker: ProcessOutput returned empty sample (total empty: {})",
                                        empty_samples
                                    );
                                }
                            }
                            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                                // すべての出力を取得した - 正常（次のNeedInputを待つ）
                                debug!("MF encoder worker: all output retrieved");
                            }
                            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                                warn!("MF encoder worker: stream change detected");
                                // ストリーム変更が発生した場合は再初期化が必要かもしれないが、
                                // ここでは警告のみ
                            }
                            Err(e) => {
                                warn!(
                                    "MF encoder worker: ProcessOutput failed: {} (code: {:?}, status: {})",
                                    e,
                                    e.code(),
                                    status
                                );
                                encode_failures += 1;
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
            "MF encoder worker: exiting (successful: {}, failures: {}, empty samples: {})",
            successful_encodes, encode_failures, empty_samples
        );
    });

    (vec![job_tx], res_rx)
}
