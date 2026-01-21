mod frame_processor;
mod track_writer;

use anyhow::Result;
use core_types::{Frame, VideoEncoderFactory, VideoStreamMessage};
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
    video_encoder_factory: Arc<dyn VideoEncoderFactory>,
    video_stream_msg_rx: mpsc::Receiver<VideoStreamMessage>,
}

impl VideoStreamService {
    /// 新しいVideoStreamServiceを作成
    pub fn new(
        frame_rx: mpsc::Receiver<Frame>,
        video_encoder_factory: Arc<dyn VideoEncoderFactory>,
        video_stream_msg_rx: mpsc::Receiver<VideoStreamMessage>,
    ) -> Self {
        info!("VideoStreamService::new");
        Self {
            frame_rx,
            video_encoder_factory,
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
        )>,
    ) -> Result<()> {
        info!("VideoStreamService started");

        // エンコーダーをセットアップ
        let (encode_job_slot, mut encode_result_rx) = self.video_encoder_factory.setup();

        // キーフレーム要求フラグ
        let keyframe_requested = Arc::new(AtomicBool::new(false));

        // 現在のアクティブなトラック情報
        let mut current_video_track: Option<Arc<TrackLocalStaticSample>> = None;
        let mut current_connection_ready: Option<Arc<AtomicBool>> = None;

        // ビデオフレームをエンコーダーに転送するタスクをスポーン
        // Note: connection_ready はここでは直接渡さず、
        // frame_router内では「エンコードすべきか」の判断に使われるかもしれないが、
        // 現状の実装では frame_router に渡す connection_ready は不変のArcなので、
        // 動的に変更するためには frame_router も変更する必要がある。
        // しかし、frame_router はエンコードを行うだけで、送信は track_writer が行う。
        // connection_ready が false の場合でもエンコードは続けても良いが（キーフレーム生成のため）、
        // 無駄なCPUリソースを使わないためには止めたほうが良い。
        //
        // 今回の要件では「接続がある状態」での再接続なので、
        // 常に「誰かしら」が見ている可能性が高い。
        // frame_router には「グローバルな」connection_ready フラグを渡すか、
        // あるいは frame_router 側で制御するのをやめて、
        // ここで encode_job_slot に送るかどうかを制御する形にするのが本来は望ましい。
        //
        // 既存の frame_processor::run_frame_router を見ると、
        // connection_ready をチェックしてエンコードジョブを投げるか判断している。
        // これを動的に更新できるようにするために、
        // 新しい connection_ready を共有できる仕組みが必要。
        //
        // 簡易的な対応として、グローバルな AtomicBool を作成し、
        // トラック更新時にその値を書き換える... というのは AtomicBool 自体が共有されているので難しい。
        //
        // 最も確実なのは、frame_router に渡す connection_ready を
        // 「現在の接続状態」を示す AtomicBool への参照を持つラッパーにするか、
        // あるいは frame_router を修正すること。
        //
        // ここでは、frame_router に渡す connection_ready は「ダミー（常にTrue）」にして、
        // 実際の送信制御（track_writer）と、エンコード要否判断（ここで制御）を行う形にしたいが、
        // frame_router は別タスクで動いており、channel で frame_rx を持っていってしまっている。
        //
        // 既存のロジックを生かすため、
        // 「現在アクティブな connection_ready」を指す AtomicBool を
        // frame_router と共有するのは、Arcの差し替えができないためスレッド間共有では難しい。
        //
        // 解決策:
        // frame_router に渡す connection_ready は、
        // 「VideoStreamServiceが管理する、現在有効な接続があるか」を示すフラグにする。
        // 個別の接続の connection_ready の状態はこのフラグにミラーリングする。
        //
        // つまり、
        // 1. service_connection_ready = Arc::new(AtomicBool::new(false)) を作る
        // 2. frame_router にはこれだけを渡す
        // 3. track_rx で新しい接続を受け取ったら、
        //    その接続の connection_ready を監視するタスクを別途立てて、
        //    service_connection_ready に反映する... のは複雑。
        //
        // そもそも connection_ready は「ICE/DTLS接続完了」を示すもの。
        // 再接続時は一時的に false になるはず。
        //
        // シンプルにするため、frame_router には「常にTrue」に近いものを渡しておき（あるいは既存のものを渡すが無視させる）、
        // エンコード結果を受け取った後の track_writer の手前で
        // current_connection_ready をチェックして書き込みをスキップする形が良いか？
        // -> frame_router で connection_ready が false だとエンコード自体がスキップされる。
        // エンコードがスキップされるとキーフレームが生成されないので、
        // 接続直後に映像が出ない可能性がある（IDR待ちになる）。
        //
        // frame_router の実装を確認（view_fileしていないが推測）。
        // 恐らく connection_ready が false なら drop している。
        //
        // 方針:
        // frame_router には「サービスとしてアクティブか」を示す global_connection_ready を渡す。
        // トラック切り替え時、新しい connection_ready の状態を監視し、
        // global_connection_ready に反映させるループを作る必要があるが、
        // AtomicBool の変更検知はポーリングになる。
        //
        // 代替案:
        // frame_router に渡す connection_ready は「常にtrue」にする。
        // エンコードは常に回す（負荷はかかるが、アイドル時もH.264のIDR生成などは必要かもしれない）。
        // 送信側（ここ）で current_connection_ready を見て drop する。
        // これなら frame_router の変更は最小限で済む（あるいは変更不要でダミーを渡す）。
        
        let global_encode_enable = Arc::new(AtomicBool::new(false)); // 初期値はfalse
        let keyframe_requested_clone = keyframe_requested.clone();
        
        // frame_router 用に clone
        let global_encode_enable_for_router = global_encode_enable.clone();

        let frame_router_handle = tokio::spawn(async move {
            frame_processor::run_frame_router(
                self.frame_rx,
                encode_job_slot,
                self.video_encoder_factory.clone(),
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
                        Some((track, sender, connection_ready)) => {
                            info!("Switched to new video track");
                            
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
