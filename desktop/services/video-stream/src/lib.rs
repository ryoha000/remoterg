mod frame_processor;
mod track_writer;

use anyhow::Result;
use core_types::{Frame, VideoCodec, VideoEncoderFactory, VideoStreamMessage};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// VideoStreamService
/// 責務: ビデオフレーム受信 → エンコード → ビデオトラック書き込み
pub struct VideoStreamService {
    frame_rx: mpsc::Receiver<Frame>,
    encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>>,
    video_stream_msg_rx: mpsc::Receiver<VideoStreamMessage>,
}

impl VideoStreamService {
    /// 新しいVideoStreamServiceを作成
    pub fn new(
        frame_rx: mpsc::Receiver<Frame>,
        encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>>,
        video_stream_msg_rx: mpsc::Receiver<VideoStreamMessage>,
    ) -> Self {
        info!("VideoStreamService::new");
        Self {
            frame_rx,
            encoder_factories,
            video_stream_msg_rx,
        }
    }

    /// サービスを実行（ブロッキング）
    /// ビデオトラックとRTPSenderを受け取り、エンコード結果を書き込む
    pub async fn run(
        mut self,
        mut track_rx: mpsc::Receiver<(
            Arc<TrackLocalStaticSample>,
            Arc<RTCRtpSender>,
            Arc<AtomicBool>, // connection_ready
            VideoCodec,
        )>,
    ) -> Result<()> {
        info!("VideoStreamService started");

        // エンコード結果を集約するチャネル
        let (encode_result_tx, mut encode_result_rx) = mpsc::unbounded_channel();

        // キーフレーム要求フラグ
        let keyframe_requested = Arc::new(AtomicBool::new(false));

        // 現在のアクティブなトラック情報
        let mut current_video_track: Option<Arc<TrackLocalStaticSample>> = None;
        let mut current_connection_ready: Option<Arc<AtomicBool>> = None;

        let global_encode_enable = Arc::new(AtomicBool::new(false)); // 初期値はfalse
        let keyframe_requested_clone = keyframe_requested.clone();

        // frame_router 用に clone
        let global_encode_enable_for_router = global_encode_enable.clone();

        // frame_router はエンコーダーファクトリが必要だが、コーデックが決まるまで開始できない可能性がある。
        // また、実行中にコーデックが変更される場合も考慮する必要がある。
        // そのため、初期ファクトリ（またはデフォルト）で開始し、
        // factory_update_tx/rx を通じて動的にファクトリを更新する仕組みを採用する。

        let (factory_update_tx, factory_update_rx) = mpsc::channel(1);

        // デフォルトファクトリ（H264があればH264、なければ適当に）
        let initial_factory = self
            .encoder_factories
            .get(&VideoCodec::H264)
            .or_else(|| self.encoder_factories.values().next())
            .expect("No encoder factories available")
            .clone();

        let frame_router_handle = tokio::spawn(async move {
            frame_processor::run_frame_router(
                self.frame_rx,
                encode_result_tx,
                initial_factory,
                factory_update_rx,
                global_encode_enable_for_router, // エンコード可否はここで制御
                keyframe_requested_clone,
            )
            .await
        });

        // 統計情報
        let mut first_encode_result_received = false;
        let mut last_encode_result_wait_start = Instant::now();
        let mut encode_result_timeout_warned = false;

        // RTCP読み込みタスクのハンドル（キャンセル用）
        let mut rtcp_drain_handle: Option<tokio::task::JoinHandle<()>> = None;

        info!("VideoStreamService entered main loop");

        loop {
            tokio::select! {
                // 1. 新しいトラック・接続情報の受信
                new_track = track_rx.recv() => {
                    match new_track {
                        Some((track, sender, connection_ready, codec)) => {
                            info!("Switched to new video track (codec: {:?})", codec);

                            // コーデックに対応するファクトリを取得
                            if let Some(factory) = self.encoder_factories.get(&codec) {
                                // ファクトリ更新を通知
                                if let Err(e) = factory_update_tx.send(factory.clone()).await {
                                    warn!("Failed to update encoder factory: {}", e);
                                } else {
                                    info!("Encoder factory updated for codec {:?}", codec);
                                }
                            } else {
                                warn!("No encoder factory found for codec {:?}, keeping current", codec);
                            }

                            // 古いRTCPタスクをキャンセル
                            if let Some(handle) = rtcp_drain_handle.take() {
                                handle.abort();
                            }

                            // 新しいRTCPタスクを起動
                            let sender_for_rtcp = sender.clone();
                            rtcp_drain_handle = Some(tokio::spawn(async move {
                                let mut rtcp_buf = vec![0u8; 1500];
                                while let Ok((_, _)) = sender_for_rtcp.read(&mut rtcp_buf).await {}
                            }));

                            // 明示的な送信開始
                            let sender_for_start = sender.clone();
                            tokio::spawn(async move {
                                let params = sender_for_start.get_parameters().await;
                                if let Err(e) = sender_for_start.send(&params).await {
                                    warn!("Video RTCRtpSender::send() failed: {}", e);
                                }
                            });

                            // ステート更新
                            current_video_track = Some(track);
                            current_connection_ready = Some(connection_ready);

                            // エンコードを有効化（再接続時は即座に有効化して良いとする）
                            // 本来は connection_ready を監視して true になったら有効化すべきだが、
                            // frame_router に渡しているのは global_encode_enable なので、
                            // これを true にすればエンコードが始まる。
                            // 実際の送信は下の encode_result 受信時に current_connection_ready を見る。
                            global_encode_enable.store(true, Ordering::Relaxed);

                            // キーフレーム要求を出して、新しい接続に即座に絵が出るようにする
                            keyframe_requested.store(true, Ordering::Relaxed);
                        }
                        None => {
                            info!("Video track channel closed");
                            break;
                        }
                    }
                }

                // 2. エンコード結果の受信と送信
                result = encode_result_rx.recv() => {
                    match result {
                        Some(encode_result) => {
                            if !first_encode_result_received {
                                info!(
                                    "First video encode result received: {} bytes, keyframe: {}",
                                    encode_result.sample_data.len(),
                                    encode_result.is_keyframe
                                );
                                first_encode_result_received = true;
                                encode_result_timeout_warned = false;
                            }

                            // 現在アクティブなトラックがあり、かつ接続準備完了していれば送信
                            if let (Some(track), Some(conn_ready)) = (&current_video_track, &current_connection_ready) {
                                if conn_ready.load(Ordering::Relaxed) {
                                     track_writer::write_encoded_sample(
                                        track,
                                        encode_result,
                                    ).await?;

                                    last_encode_result_wait_start = Instant::now();
                                } else {
                                    // 接続準備未完了ならドロップ（ログ出しすぎないよう注意）
                                    // debug!("Connection not ready, dropping video frame");
                                }
                            }
                        }
                        None => {
                            info!("Video encode result channel closed");
                            break;
                        }
                    }
                }

                // 3. キーフレーム要求
                msg = self.video_stream_msg_rx.recv() => {
                    match msg {
                        Some(VideoStreamMessage::RequestKeyframe) => {
                            debug!("Received keyframe request");
                            keyframe_requested.store(true, Ordering::Relaxed);
                        }
                        None => {
                            info!("Video stream message channel closed");
                            break;
                        }
                    }
                }

                // 4. タイムアウト監視
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(3)) => {
                    if !first_encode_result_received {
                        // まだ一度も受信していない場合
                         if let Some(conn_ready) = &current_connection_ready {
                            if conn_ready.load(Ordering::Relaxed) {
                                let wait_duration = last_encode_result_wait_start.elapsed();
                                if wait_duration.as_secs() >= 3 && !encode_result_timeout_warned {
                                    warn!(
                                        "No encode result received for {}s (connection_ready: true)",
                                        wait_duration.as_secs()
                                    );
                                    encode_result_timeout_warned = true;
                                }
                            }
                         }
                    }
                }
            }
        }

        // クリーンアップ
        if let Some(handle) = rtcp_drain_handle {
            handle.abort();
        }
        let _ = frame_router_handle.await;

        info!("VideoStreamService stopped");
        Ok(())
    }
}
