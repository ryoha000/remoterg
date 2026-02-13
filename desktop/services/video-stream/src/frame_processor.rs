use core_types::{EncodeJob, EncodeJobSlot, Frame, VideoEncoderFactory};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, span, warn, Level};

/// フレーム処理の統計情報
struct FrameStats {
    frames_received: u64,
    frames_dropped_not_ready: u64,
    frames_dropped_no_encoder: u64,
    frames_queued: u64,
    last_perf_log: Instant,
}

impl FrameStats {
    fn new() -> Self {
        Self {
            frames_received: 0,
            frames_dropped_not_ready: 0,
            frames_dropped_no_encoder: 0,
            frames_queued: 0,
            last_perf_log: Instant::now(),
        }
    }

    fn log_if_needed(&mut self) {
        if self.last_perf_log.elapsed().as_secs_f32() >= 5.0 {
            let elapsed_sec = self.last_perf_log.elapsed().as_secs_f32();
            let receive_fps = self.frames_received as f32 / elapsed_sec;
            let queue_fps = self.frames_queued as f32 / elapsed_sec;
            tracing::info!(
                "Frame processing stats (last {}s): received={} ({:.1} fps), queued={} ({:.1} fps), dropped_not_ready={}, dropped_no_encoder={}",
                elapsed_sec,
                self.frames_received,
                receive_fps,
                self.frames_queued,
                queue_fps,
                self.frames_dropped_not_ready,
                self.frames_dropped_no_encoder
            );
            self.frames_received = 0;
            self.frames_queued = 0;
            self.frames_dropped_not_ready = 0;
            self.frames_dropped_no_encoder = 0;
            self.last_perf_log = Instant::now();
        }
    }
}

/// フレームルーター: フレームをエンコーダーに転送する非同期タスク
pub async fn run_frame_router(
    mut frame_rx: tokio::sync::mpsc::Receiver<Frame>,
    result_tx: tokio::sync::mpsc::UnboundedSender<core_types::EncodeResult>,
    mut encoder_factory: Arc<dyn VideoEncoderFactory>,
    mut factory_update_rx: tokio::sync::mpsc::Receiver<Arc<dyn VideoEncoderFactory>>,
    connection_ready: Arc<AtomicBool>,
    keyframe_requested: Arc<AtomicBool>,
) {
    info!("Frame router started");

    let mut encode_job_slot: Option<Arc<EncodeJobSlot>> = None;
    let mut current_width: u32 = 0;
    let mut current_height: u32 = 0;
    let mut last_frame_ts: Option<u64> = None;
    let mut stats = FrameStats::new();
    let mut first_frame_received = false;
    let mut first_job_queued = false;

    loop {
        tokio::select! {
            // 1. フレーム受信
            frame_res = frame_rx.recv() => {
                match frame_res {
                    Some(frame) => {
                         let pipeline_start = Instant::now();
                        stats.frames_received += 1;

                        let interarrival_ms = last_frame_ts
                            .map(|prev| {
                                // windows_timespan は100ナノ秒単位なので、ミリ秒に変換
                                let delta_hns = frame.windows_timespan.saturating_sub(prev);
                                delta_hns / 10_000
                            })
                            .unwrap_or(0);

                        if !first_frame_received {
                            info!(
                                "First frame received: {}x{} (connection_ready: {})",
                                frame.width,
                                frame.height,
                                connection_ready.load(Ordering::Relaxed)
                            );
                            first_frame_received = true;
                        }

                        debug!(
                            "Received frame: {}x{} (since_last={}ms)",
                            frame.width, frame.height, interarrival_ms
                        );

                        // ICE/DTLS 接続完了まで映像送出を保留
                        if !connection_ready.load(Ordering::Relaxed) {
                            stats.frames_dropped_not_ready += 1;
                            if stats.frames_dropped_not_ready == 1 || stats.frames_dropped_not_ready % 100 == 0 {
                                warn!(
                                    "Connection not ready yet, dropped {} frames (connection_ready: false)",
                                    stats.frames_dropped_not_ready
                                );
                            }
                            tracing::trace!(name: "frame_drop", reason = "connection_not_ready", frame_id = frame.id);
                            continue;
                        }

                        // フレーム処理全体を span で計測
                        let process_frame_span = span!(
                            Level::DEBUG,
                            "process_frame",
                            width = frame.width,
                            height = frame.height,
                            frame_id = frame.id
                        );
                        let _process_frame_guard = process_frame_span.enter();

                        // タイムスタンプを更新
                        last_frame_ts = Some(frame.windows_timespan);

                        // 解像度変更を検出した場合はencoderを再生成
                        let resolution_changed = current_width != frame.width || current_height != frame.height;
                        if resolution_changed {
                            if current_width == 0 && current_height == 0 {
                                // 最初のフレーム: エンコーダーは既に起動済みで最初のフレームを待機中
                                // shutdownせずに解像度を更新するだけ
                                info!(
                                    "Observed first frame {}x{} (encoder already initialized and waiting)",
                                    frame.width, frame.height
                                );
                                current_width = frame.width;
                                current_height = frame.height;

                                // 初回起動時もエンコーダーを作成・転送タスク起動が必要
                                // encode_job_slot が空の場合のみ（初回）このパスに来る。

                                if encode_job_slot.is_none() {
                                    info!("Initializing encoder for the first time");
                                    let (new_slot, mut new_rx) = encoder_factory.setup();

                                    // 結果転送タスクを起動
                                    let result_tx_clone = result_tx.clone();
                                    tokio::spawn(async move {
                                        while let Some(res) = new_rx.recv().await {
                                            if result_tx_clone.send(res).is_err() {
                                                break;
                                            }
                                        }
                                    });

                                    encode_job_slot = Some(new_slot);
                                }

                                // 最初のキーフレームを要求
                                keyframe_requested.store(true, Ordering::Relaxed);
                            } else {
                                // 実際の解像度変更: エンコーダーを再起動
                                info!(
                                    "Observed frame resize {}x{} -> {}x{} (recreating encoder)",
                                    current_width, current_height, frame.width, frame.height
                                );

                                // 既存のencoderワーカーを停止
                                if let Some(old_slot) = encode_job_slot.as_ref() {
                                    old_slot.shutdown();
                                }
                                drop(encode_job_slot.take());

                                // 新しいencoderワーカーを起動
                                let (new_slot, mut new_rx) = encoder_factory.setup();

                                // 結果転送タスクを起動
                                let result_tx_clone = result_tx.clone();
                                tokio::spawn(async move {
                                    while let Some(res) = new_rx.recv().await {
                                        if result_tx_clone.send(res).is_err() {
                                            break;
                                        }
                                    }
                                });

                                encode_job_slot = Some(new_slot);

                                current_width = frame.width;
                                current_height = frame.height;
                                keyframe_requested.store(true, Ordering::Relaxed);
                            }
                        }

                        // エンコードジョブ送信を span で計測
                        if let Some(job_slot) = encode_job_slot.as_ref() {
                            let queue_encode_job_span =
                                span!(Level::DEBUG, "queue_encode_job", frame_id = frame.id);
                            let _queue_encode_job_guard = queue_encode_job_span.enter();
                            let job_send_start = Instant::now();

                            // キーフレーム要求が来ている場合は、フラグをリセットしてジョブに含める
                            let request_keyframe = keyframe_requested.swap(false, Ordering::Relaxed);

                            if !first_job_queued {
                                info!(
                                    "Queueing first encode job: {}x{} (keyframe: {})",
                                    frame.width, frame.height, request_keyframe
                                );
                                first_job_queued = true;
                            }

                            job_slot.set(EncodeJob {
                                width: frame.width,
                                height: frame.height,
                                timestamp: frame.windows_timespan,
                                enqueue_at: pipeline_start,
                                request_keyframe,
                                frame_id: frame.id,
                                texture_handle: frame.texture_handle,
                            });

                            let job_send_dur = job_send_start.elapsed();
                            drop(_queue_encode_job_guard);

                            stats.frames_queued += 1;
                            if job_send_dur.as_millis() > 10 {
                                warn!("Encode job set took {}ms", job_send_dur.as_millis());
                            }
                        } else {
                            stats.frames_dropped_no_encoder += 1;
                            if stats.frames_dropped_no_encoder == 1 || stats.frames_dropped_no_encoder % 10 == 0 {
                                warn!(
                                    "Encoder worker not available, dropped {} frames",
                                    stats.frames_dropped_no_encoder
                                );
                            }
                            tracing::trace!(name: "frame_drop", reason = "no_encoder", frame_id = frame.id);
                        }

                        drop(_process_frame_guard);

                        // パフォーマンス統計を定期的に出力
                        stats.log_if_needed();
                    }
                    None => {
                        info!("Frame channel closed");
                        break;
                    }
                }
            }

            // 2. エンコーダーファクトリ更新
            new_factory = factory_update_rx.recv() => {
                match new_factory {
                    Some(factory) => {
                        info!("VideoEncoderFactory updated (codec: {:?})", factory.codec());
                        encoder_factory = factory;

                        // 既存のエンコーダーを停止して再初期化を強制する
                        if let Some(old_slot) = encode_job_slot.as_ref() {
                             old_slot.shutdown();
                        }
                        drop(encode_job_slot.take());

                        // エンコーダーを即座に再起動（現在の解像度を使用）
                        if current_width > 0 && current_height > 0 {
                            info!("Re-initializing encoder with new factory for {}x{}", current_width, current_height);
                            let (new_slot, mut new_rx) = encoder_factory.setup();

                            let result_tx_clone = result_tx.clone();
                            tokio::spawn(async move {
                                while let Some(res) = new_rx.recv().await {
                                    if result_tx_clone.send(res).is_err() {
                                        break;
                                    }
                                }
                            });

                            encode_job_slot = Some(new_slot);
                            // コーデック変更後はキーフレーム必須
                            keyframe_requested.store(true, Ordering::Relaxed);
                        } else {
                            // まだ解像度が決まっていない場合は、次のフレーム受信時に初期化されるようにする
                            // (current_width/height が 0 なので、最初のフレームとして扱われる)
                            info!("Encoder factory updated, waiting for first frame to initialize");
                        }
                    }
                    None => {
                        // ストリームが終了することはないはずだが、クローズされたらループ継続（更新なし）
                        // あるいは break するか？ ここでは無視して pending にする
                        // しかし recv() が None を返すと select! が即座に回り続けてしまうので対策が必要
                         // factory_update_rx を無効化する
                         // ここではダミーの future を待つようにするなど工夫が必要だが、
                         // VideoStreamService が生きている限り閉じないはず。
                         // 念のため break
                         info!("Encoder factory update channel closed");
                         // これ以降更新を受け取れないだけなので break はしないが、
                         // select! 内で None を引き続けるのを防ぐ必要がある。
                         // 本来は loop の外で Option<Receiver> にして、None なら pending にする。
                         // 簡易的に break してしまうことにする（異常系）
                         break;
                    }
                }
            }
        }
    }

    // クリーンアップ: エンコーダーをシャットダウン
    if let Some(job_slot) = encode_job_slot.as_ref() {
        job_slot.shutdown();
    }

    info!("Frame router stopped");
}
