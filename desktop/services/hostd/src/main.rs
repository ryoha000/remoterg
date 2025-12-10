use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::pin;
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[cfg(not(feature = "mock"))]
use capture::CaptureService;
#[cfg(feature = "mock")]
use capturemock::CaptureService;
use core_types::{
    CaptureBackend, CaptureMessage, DataChannelMessage, Frame, SignalingResponse, VideoCodec,
    VideoEncoderFactory,
};
#[cfg(feature = "h264")]
use encoder::openh264::OpenH264EncoderFactory;
#[cfg(feature = "vp8")]
use encoder::vp8::Vp8EncoderFactory;
#[cfg(feature = "vp9")]
use encoder::vp9::Vp9EncoderFactory;
use input::InputService;
use signaling::SignalingService;
use webrtc::WebRtcService;

#[derive(Parser, Debug)]
#[command(name = "hostd")]
#[command(about = "RemoteRG Host Daemon")]
struct Args {
    /// HTTP/WebSocket server port
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Capture target window handle (HWND)
    #[arg(long, default_value_t = 0)]
    hwnd: u64,
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
    info!("Port: {}, Log Level: {}", args.port, args.log_level);
    info!("Capture HWND: {}", hwnd);

    // チャンネル作成
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>(100);
    let (capture_cmd_tx, capture_cmd_rx) = mpsc::channel::<CaptureMessage>(10);
    let (signaling_response_tx, signaling_response_rx) = mpsc::channel::<SignalingResponse>(100);
    let (data_channel_tx, data_channel_rx) = mpsc::channel::<DataChannelMessage>(100);

    #[cfg(all(not(feature = "vp9"), not(feature = "vp8"), not(feature = "h264")))]
    compile_error!("At least one of vp9, vp8, or h264 feature must be enabled for hostd");

    let mut encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>> = HashMap::new();
    #[cfg(feature = "vp9")]
    {
        encoder_factories.insert(VideoCodec::Vp9, Arc::new(Vp9EncoderFactory::new()));
    }
    #[cfg(feature = "vp8")]
    {
        encoder_factories.insert(VideoCodec::Vp8, Arc::new(Vp8EncoderFactory::new()));
    }
    #[cfg(feature = "h264")]
    {
        encoder_factories.insert(VideoCodec::H264, Arc::new(OpenH264EncoderFactory::new()));
    }

    // サービス作成
    let capture_service = CaptureService::new(frame_tx, capture_cmd_rx);
    let (webrtc_service, webrtc_msg_tx) = WebRtcService::new(
        frame_rx,
        signaling_response_tx,
        data_channel_tx,
        encoder_factories,
    );
    let input_service = InputService::new(data_channel_rx);
    let signaling_service = SignalingService::new(args.port, webrtc_msg_tx, signaling_response_rx);

    // CaptureServiceを開始
    capture_cmd_tx
        .send(CaptureMessage::Start { hwnd })
        .await
        .context("Failed to start capture service")?;
    if cfg!(feature = "mock") {
        info!("CaptureService started (mock frames)");
    } else {
        info!("CaptureService started (real capture)");
    }

    // サービスを独立タスクとして起動（Send でない WebRTC はこのスレッドで駆動する）
    let capture_handle = tokio::spawn(async move { capture_service.run().await });
    let input_handle = tokio::spawn(async move { input_service.run().await });
    let signaling_handle = tokio::spawn(async move { signaling_service.run().await });
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
