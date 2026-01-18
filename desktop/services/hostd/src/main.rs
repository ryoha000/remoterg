use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::pin;
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use audio_capture;
use audio_capture_mock;
use audio_encoder::OpusEncoderFactory;
use audio_stream::AudioStreamService;
use core_types::{
    AudioCaptureMessage, AudioFrame, CaptureBackend, CaptureMessage, DataChannelMessage, Frame,
    SignalingResponse, VideoCodec, VideoEncoderFactory, VideoStreamMessage,
};
#[cfg(feature = "h264")]
use encoder::h264::mmf::MediaFoundationH264EncoderFactory;
use input::InputService;
use signaling::SignalingClient;
use video_capture;
use video_capture_mock;
use video_stream::VideoStreamService;
use webrtc::WebRtcService;
use tagger::TaggerService;
use tagger_setup::TaggerSetup;

#[derive(Parser, Debug)]
#[command(name = "hostd")]
#[command(about = "RemoteRG Host Daemon")]
struct Args {
    /// Cloudflare WebSocket URL (e.g., wss://example.com/api/signal)
    #[arg(long, default_value = "ws://localhost:3000/api/signal")]
    cloudflare_url: String,

    /// Session ID
    #[arg(long, default_value = "fixed")]
    session_id: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Capture target window handle (HWND)
    #[arg(long, default_value_t = 0)]
    hwnd: u64,

    /// Use mock implementations for video and audio capture
    #[arg(long)]
    mock: bool,

    /// Port for local LLM server (llama-server)
    #[arg(long, default_value_t = 8081)]
    llm_port: u16,
}

enum CaptureServiceEnum {
    Real(video_capture::CaptureService),
    Mock(video_capture_mock::CaptureService),
}

impl CaptureServiceEnum {
    async fn run(self) -> Result<()> {
        match self {
            CaptureServiceEnum::Real(service) => service.run().await,
            CaptureServiceEnum::Mock(service) => service.run().await,
        }
    }
}

enum AudioCaptureServiceEnum {
    Real(audio_capture::AudioCaptureService),
    Mock(audio_capture_mock::AudioCaptureService),
}

impl AudioCaptureServiceEnum {
    async fn run(self) -> Result<()> {
        match self {
            AudioCaptureServiceEnum::Real(service) => service.run().await,
            AudioCaptureServiceEnum::Mock(service) => service.run().await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // ログ設定
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // hwndが指定されていない場合、環境変数から読み取る
    let hwnd = if args.hwnd == 0 {
        std::env::var("REMOTERG_HWND")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        args.hwnd
    };

    info!("Starting RemoteRG Host Daemon");
    info!(
        "Cloudflare URL: {}, Session ID: {}",
        args.cloudflare_url, args.session_id
    );
    info!("Log Level: {}", args.log_level);
    info!("Capture HWND: {}", hwnd);
    info!("LLM Port: {}", args.llm_port);

    // LLM Sidecar Setup
    let mut tagger_setup = TaggerSetup::new();
    if let Err(e) = tagger_setup.start(args.llm_port).await {
        tracing::warn!("Failed to start LLM sidecar: {}", e);
    }
    let tagger_service = TaggerService::new(args.llm_port);

    // チャンネル作成
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>(100);
    let (capture_cmd_tx, capture_cmd_rx) = mpsc::channel::<CaptureMessage>(10);
    let (signaling_response_tx, signaling_response_rx) = mpsc::channel::<SignalingResponse>(100);
    let (data_channel_tx, data_channel_rx) = mpsc::channel::<DataChannelMessage>(100);

    // 音声チャンネル作成
    let (audio_capture_cmd_tx, audio_capture_cmd_rx) = mpsc::channel::<AudioCaptureMessage>(10);

    // ビデオストリームメッセージチャネル（キーフレーム要求など）
    let (video_stream_msg_tx, video_stream_msg_rx) = mpsc::channel::<VideoStreamMessage>(10);

    // ビデオトラック情報を受け渡すためのチャンネル
    let (video_track_tx, mut video_track_rx) = mpsc::channel::<(
        Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
        Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
        Arc<std::sync::atomic::AtomicBool>, // connection_ready
    )>(10);

    // 音声トラック情報を受け渡すためのチャンネル
    let (audio_track_tx, mut audio_track_rx) = mpsc::channel::<(
        Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
        Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
    )>(10);

    #[cfg(not(feature = "h264"))]
    compile_error!("h264 feature must be enabled for hostd");

    let mut encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>> = HashMap::new();
    #[cfg(feature = "h264")]
    {
        encoder_factories.insert(
            VideoCodec::H264,
            // Arc::new(OpenH264EncoderFactory::new()),
            Arc::new(MediaFoundationH264EncoderFactory::new()),
        );
    }

    // 音声フレーム用のチャンネルを作成
    let (audio_frame_tx, audio_frame_rx) = mpsc::channel::<AudioFrame>(100);

    // デフォルトのビデオエンコーダーを選択
    let default_video_encoder = encoder_factories
        .get(&VideoCodec::H264)
        .expect("H264 encoder must be available")
        .clone();

    // 音声エンコーダーファクトリを作成
    let audio_encoder_factory = Arc::new(OpusEncoderFactory::new());

    // サービス作成
    let capture_service = if args.mock {
        CaptureServiceEnum::Mock(video_capture_mock::CaptureService::new(
            frame_tx,
            capture_cmd_rx,
        ))
    } else {
        CaptureServiceEnum::Real(video_capture::CaptureService::new(frame_tx, capture_cmd_rx))
    };
    let audio_capture_service = if args.mock {
        AudioCaptureServiceEnum::Mock(audio_capture_mock::AudioCaptureService::new(
            audio_frame_tx,
            audio_capture_cmd_rx,
        ))
    } else {
        AudioCaptureServiceEnum::Real(audio_capture::AudioCaptureService::new(
            audio_frame_tx,
            audio_capture_cmd_rx,
        ))
    };
    // VideoStreamService を作成
    let video_stream_service =
        VideoStreamService::new(frame_rx, default_video_encoder, video_stream_msg_rx);

    // WebRTCサービスの起動
    // Outgoing DataChannelメッセージ用チャネル (InputService -> WebRtcService)
    let (outgoing_dc_tx, outgoing_dc_rx) = mpsc::channel(100);

    let (webrtc_service, webrtc_msg_tx) = WebRtcService::new(
        signaling_response_tx,
        data_channel_tx,
        Some(outgoing_dc_rx), // Pass outgoing_dc_rx
        Some(video_track_tx),
        Some(video_stream_msg_tx.clone()), // Use clone of video_stream_msg_tx
        Some(audio_track_tx),
    );

    // WebRtcService::run() に渡すために webrtc_msg_tx をクローン
    let webrtc_msg_tx_for_run = webrtc_msg_tx.clone();

    let audio_stream_service = AudioStreamService::new(audio_frame_rx, audio_encoder_factory);

    // CaptureServiceへのコマンド送信チャネルを複製
    let capture_cmd_tx_for_input = capture_cmd_tx.clone();
    
    let input_service = InputService::new(
        data_channel_rx, 
        capture_cmd_tx_for_input, 
        outgoing_dc_tx, // Pass outgoing_dc_tx
        tagger_service,
    );
    let signaling_client = SignalingClient::new(
        args.cloudflare_url,
        args.session_id,
        webrtc_msg_tx,
        signaling_response_rx,
    );

    // CaptureServiceを開始
    capture_cmd_tx
        .send(CaptureMessage::Start { hwnd })
        .await
        .context("Failed to start capture service")?;
    if args.mock {
        info!("CaptureService started (mock frames)");
    } else {
        info!("CaptureService started (real capture)");
    }

    // AudioCaptureServiceを開始
    audio_capture_cmd_tx
        .send(AudioCaptureMessage::Start { hwnd })
        .await
        .context("Failed to start audio capture service")?;
    if args.mock {
        info!("AudioCaptureService started (mock audio)");
    } else {
        info!("AudioCaptureService started (real audio)");
    }

    // サービスを独立タスクとして起動（Send でない WebRTC はこのスレッドで駆動する）
    let capture_handle = tokio::spawn(async move { capture_service.run().await });
    let audio_capture_handle = tokio::spawn(async move { audio_capture_service.run().await });
    let input_handle = tokio::spawn(async move { input_service.run().await });
    let signaling_handle = tokio::spawn(async move { signaling_client.run().await });

    // VideoStreamService起動タスク
    let video_stream_handle = tokio::spawn(async move {
        video_stream_service.run(video_track_rx).await
    });

    // AudioStreamService起動タスク
    let audio_stream_handle = tokio::spawn(async move {
        audio_stream_service.run(audio_track_rx).await
    });

    // WebRTC は非 Send 型を含むため spawn せず現在のタスクで実行する
    let webrtc_fut = webrtc_service.run(webrtc_msg_tx_for_run);
    pin!(webrtc_fut);

    tokio::select! {
        result = &mut webrtc_fut => match result {
            Ok(()) => info!("WebRtcService finished"),
            Err(e) => tracing::error!("WebRtcService error: {}", e),
        },
        result = capture_handle => match result {
            Ok(Ok(())) => info!("CaptureService finished"),
            Ok(Err(e)) => tracing::error!("CaptureService error: {}", e),
            Err(e) => tracing::error!("CaptureService task panicked: {}", e),
        },
        result = audio_capture_handle => match result {
            Ok(Ok(())) => info!("AudioCaptureService finished"),
            Ok(Err(e)) => tracing::error!("AudioCaptureService error: {}", e),
            Err(e) => tracing::error!("AudioCaptureService task panicked: {}", e),
        },
        result = video_stream_handle => match result {
            Ok(Ok(())) => info!("VideoStreamService finished"),
            Ok(Err(e)) => tracing::error!("VideoStreamService error: {}", e),
            Err(e) => tracing::error!("VideoStreamService task panicked: {}", e),
        },
        result = audio_stream_handle => match result {
            Ok(Ok(())) => info!("AudioStreamService finished"),
            Ok(Err(e)) => tracing::error!("AudioStreamService error: {}", e),
            Err(e) => tracing::error!("AudioStreamService task panicked: {}", e),
        },
        result = input_handle => match result {
            Ok(Ok(())) => info!("InputService finished"),
            Ok(Err(e)) => tracing::error!("InputService error: {}", e),
            Err(e) => tracing::error!("InputService task panicked: {}", e),
        },
        result = signaling_handle => match result {
            Ok(Ok(())) => info!("SignalingService finished"),
            Ok(Err(e)) => tracing::error!("SignalingService error: {}", e),
            Err(e) => tracing::error!("SignalingService task panicked: {}", e),
        },
    };

    info!("Host daemon stopped");
    Ok(())
}
