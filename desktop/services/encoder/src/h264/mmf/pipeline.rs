use core_types::{EncodeJobSlot, EncodeResult, ShutdownError};
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, info, warn};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    METransformHaveOutput, METransformNeedInput, MFCreateDXGISurfaceBuffer, MFCreateSample,
    MFSampleExtension_CleanPoint, MFSampleExtension_VideoEncodePictureType, MFT_OUTPUT_DATA_BUFFER,
    MF_EVENT_FLAG_NONE, MF_EVENT_TYPE, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE,
};

use crate::h264::mmf::d3d::D3D11Resources;
use crate::h264::mmf::encoder::H264Encoder;
use crate::h264::mmf::preprocessor::VideoProcessorPreprocessor;

/// H.264データがAnnex-B形式（スタートコード）かどうかを判定
fn is_annexb_format(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    // 4バイトスタートコード (00 00 00 01)
    if data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x00 && data[3] == 0x01 {
        return true;
    }
    // 3バイトスタートコード (00 00 01)
    if data.len() >= 3 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01 {
        return true;
    }
    false
}

/// H.264データをAnnex-B形式に変換（フォーマット自動判定）
/// 戻り値: (Annex-B形式のデータ, SPS/PPSが含まれているか)
fn annexb_from_mf_data(data: &[u8]) -> (Vec<u8>, bool) {
    const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
    let mut result = Vec::new();
    let mut has_sps_pps = false;

    // 既にAnnex-B形式の場合はそのまま返す
    if is_annexb_format(data) {
        // Annex-B形式のまま処理（NALユニットを分割してSPS/PPSを検出）
        let mut i = 0;
        while i < data.len() {
            // スタートコードを探す
            let start_code_len = if i + 4 <= data.len()
                && data[i] == 0x00
                && data[i + 1] == 0x00
                && data[i + 2] == 0x00
                && data[i + 3] == 0x01
            {
                4
            } else if i + 3 <= data.len()
                && data[i] == 0x00
                && data[i + 1] == 0x00
                && data[i + 2] == 0x01
            {
                3
            } else {
                // スタートコードが見つからない場合は残りをコピーして終了
                if i < data.len() {
                    result.extend_from_slice(&data[i..]);
                }
                break;
            };

            // 次のスタートコードを探す
            let mut next_start = None;
            let mut search_pos = i + start_code_len;
            while search_pos + 3 <= data.len() {
                if search_pos + 4 <= data.len()
                    && data[search_pos] == 0x00
                    && data[search_pos + 1] == 0x00
                    && data[search_pos + 2] == 0x00
                    && data[search_pos + 3] == 0x01
                {
                    next_start = Some((search_pos, 4));
                    break;
                } else if data[search_pos] == 0x00
                    && data[search_pos + 1] == 0x00
                    && data[search_pos + 2] == 0x01
                {
                    next_start = Some((search_pos, 3));
                    break;
                }
                search_pos += 1;
            }

            let nal_end = next_start.unwrap_or((data.len(), 0)).0;
            let nal_unit = &data[i..nal_end];

            // NALユニットのタイプを確認（SPS/PPS判定）
            if nal_unit.len() > start_code_len {
                let nal_header = nal_unit[start_code_len];
                let nal_type = nal_header & 0x1F;
                if nal_type == 7 || nal_type == 8 {
                    has_sps_pps = true;
                    debug!(
                        "MF encoder: found SPS/PPS in Annex-B data (type={})",
                        nal_type
                    );
                }
            }

            result.extend_from_slice(nal_unit);
            i = nal_end;
        }

        return (result, has_sps_pps);
    }

    // AVCC形式（NAL長プレフィックス）として処理
    debug!("MF encoder: detected AVCC format, converting to Annex-B");
    let mut i = 0;
    while i < data.len() {
        if i + 4 <= data.len() {
            // NAL長を読み取る（ビッグエンディアン）
            let nal_length =
                u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;

            i += 4;

            if i + nal_length <= data.len() && nal_length > 0 {
                let nal_unit = &data[i..i + nal_length];

                // NALユニットのタイプを確認（SPS/PPS判定）
                if nal_unit.len() > 0 {
                    let nal_type = nal_unit[0] & 0x1F;
                    if nal_type == 7 || nal_type == 8 {
                        has_sps_pps = true;
                        debug!("MF encoder: found SPS/PPS in AVCC data (type={})", nal_type);
                    }
                }

                // スタートコードを追加
                result.extend_from_slice(START_CODE);
                result.extend_from_slice(nal_unit);

                i += nal_length;
            } else {
                // 無効なNAL長の場合は残りをコピーして終了
                if i < data.len() {
                    warn!("MF encoder: invalid NAL length, copying remaining data");
                    result.extend_from_slice(&data[i..]);
                }
                break;
            }
        } else {
            // データが不足している場合は残りをコピー
            if i < data.len() {
                result.extend_from_slice(&data[i..]);
            }
            break;
        }
    }

    (result, has_sps_pps)
}

/// 入力フレームのメタ情報（出力と対応付けるため）
struct InputFrameMeta {
    duration: Duration,
    width: u32,
    height: u32,
}

/// Media Foundationエンコードワーカーを起動
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

        // codec configからSPS/PPSを取得（best-effort、取得できない場合はNone）
        let mut codec_config_sps_pps = encoder.get_codec_config();
        if codec_config_sps_pps.is_some() {
            info!("MF encoder worker: extracted SPS/PPS from codec config");
        } else {
            debug!("MF encoder worker: codec config not available, will rely on in-band SPS/PPS");
        }

        // ストリーミングを開始
        if let Err(e) = encoder.start_streaming() {
            warn!("MF encoder worker: failed to start streaming: {}", e);
            return;
        }

        // 最初のフレームを処理
        let mut pending_job = Some(first_job);
        let mut first_keyframe_sent = false;

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

                        let recv_at = Instant::now();
                        let queue_wait_dur = recv_at.duration_since(job.enqueue_at);
                        let job_width = (job.width / 2) * 2;
                        let job_height = (job.height / 2) * 2;

                        // 前処理（RGBA → NV12 テクスチャ）
                        let preprocess_start = Instant::now();
                        let nv12_texture =
                            match preprocessor.process(&job.rgba, width, height, frame_timestamp) {
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

                                    // Annex-B形式に変換（フォーマット自動判定）
                                    let pack_start = Instant::now();
                                    let (mut sample_data, has_sps_pps_in_data) =
                                        annexb_from_mf_data(&encoded_data);
                                    let pack_dur = pack_start.elapsed();

                                    // キーフレーム判定（MFSampleExtension_CleanPoint + SPS/PPS検出）
                                    let is_clean_point =
                                        match sample.GetUINT32(&MFSampleExtension_CleanPoint) {
                                            Ok(1) => true,
                                            Ok(0) => false,
                                            _ => false, // エラーまたは未設定の場合はfalse
                                        };
                                    // SPS/PPSが含まれている場合もキーフレームとして扱う（ブラウザがデコード開始できるように）
                                    let mut is_keyframe = is_clean_point || has_sps_pps_in_data;

                                    // in-bandにSPS/PPSが無く、codec configから取得したSPS/PPSがある場合、最初のキーフレームに注入
                                    if !has_sps_pps_in_data && is_keyframe && !first_keyframe_sent {
                                        if let Some((ref sps, ref pps)) = codec_config_sps_pps {
                                            debug!("MF encoder: injecting SPS/PPS from codec config (SPS: {} bytes, PPS: {} bytes)", sps.len(), pps.len());
                                            const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
                                            let mut injected_data = Vec::with_capacity(
                                                START_CODE.len()
                                                    + sps.len()
                                                    + START_CODE.len()
                                                    + pps.len()
                                                    + sample_data.len(),
                                            );
                                            injected_data.extend_from_slice(START_CODE);
                                            injected_data.extend_from_slice(sps.as_slice());
                                            injected_data.extend_from_slice(START_CODE);
                                            injected_data.extend_from_slice(pps.as_slice());
                                            injected_data.extend_from_slice(&sample_data);
                                            sample_data = injected_data;
                                            is_keyframe = true; // 注入後は確実にキーフレーム
                                            first_keyframe_sent = true;
                                        }
                                    } else if has_sps_pps_in_data {
                                        debug!("MF encoder: detected SPS/PPS in encoded data, marking as keyframe");
                                        first_keyframe_sent = true;
                                    }

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
                                            is_keyframe: is_keyframe,
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

    (job_slot, res_rx)
}
