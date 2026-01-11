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
use capture;
use capturemock;
use core_types::{
    AudioCaptureMessage, AudioFrame, CaptureBackend, CaptureMessage, DataChannelMessage, Frame,
    SignalingResponse, VideoCodec, VideoEncoderFactory,
};
#[cfg(feature = "h264")]
use encoder::h264::mmf::MediaFoundationH264EncoderFactory;
use input::InputService;
use signaling::SignalingClient;
use webrtc::WebRtcService;

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
}

enum CaptureServiceEnum {
    Real(capture::CaptureService),
    Mock(capturemock::CaptureService),
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

    // チャンネル作成
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>(100);
    let (capture_cmd_tx, capture_cmd_rx) = mpsc::channel::<CaptureMessage>(10);
    let (signaling_response_tx, signaling_response_rx) = mpsc::channel::<SignalingResponse>(100);
    let (data_channel_tx, data_channel_rx) = mpsc::channel::<DataChannelMessage>(100);

    // 音声チャンネル作成
    let (audio_capture_cmd_tx, audio_capture_cmd_rx) = mpsc::channel::<AudioCaptureMessage>(10);

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

    // 音声エンコーダーファクトリを作成（WebRtcServiceに渡すのみ）
    let audio_encoder_factory = Arc::new(OpusEncoderFactory::new());

    // サービス作成
    let capture_service = if args.mock {
        CaptureServiceEnum::Mock(capturemock::CaptureService::new(frame_tx, capture_cmd_rx))
    } else {
        CaptureServiceEnum::Real(capture::CaptureService::new(frame_tx, capture_cmd_rx))
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
    let (webrtc_service, webrtc_msg_tx) = WebRtcService::new(
        frame_rx,
        signaling_response_tx,
        data_channel_tx,
        encoder_factories,
        Some((audio_frame_rx, audio_encoder_factory)),
    );
    let input_service = InputService::new(data_channel_rx);
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
    // WebRTC は OpenH264 の非 Send 型を含むため spawn せず現在のタスクで実行する
    let webrtc_fut = webrtc_service.run();
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
