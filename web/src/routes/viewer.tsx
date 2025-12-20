import { createFileRoute } from "@tanstack/react-router";
import { useState, useRef, useEffect } from "react";
import { useWebRTC } from "@/lib/use-webrtc";

export const Route = createFileRoute("/viewer")({
  component: ViewerPage,
});

function ViewerPage() {
  const [sessionId, setSessionId] = useState<string>("fixed");
  const [codec, setCodec] = useState<"h264" | "any">("h264");
  const [stream, setStream] = useState<MediaStream | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);

  const {
    connectionState,
    iceConnectionState,
    stats,
    logs,
    connect,
    disconnect,
    sendKey,
    requestScreenshot,
  } = useWebRTC({
    signalUrl: "/api/signal",
    sessionId,
    codec,
    onTrack: (receivedStream) => {
      setStream(receivedStream);
      if (videoRef.current) {
        videoRef.current.srcObject = receivedStream;
        videoRef.current.play().catch((error) => {
          console.error("Video play error:", error);
        });
      }
    },
  });

  useEffect(() => {
    if (stream && videoRef.current) {
      videoRef.current.srcObject = stream;
    }
  }, [stream]);

  const getStatusClass = () => {
    if (connectionState === "connected") return "connected";
    if (connectionState === "error" || connectionState === "failed")
      return "error";
    return "";
  };

  const getStatusText = () => {
    if (connectionState === "connecting") return "接続中...";
    if (connectionState === "connected") return "接続済み";
    if (connectionState === "error") return "接続エラー";
    if (connectionState === "failed") return "接続失敗";
    if (connectionState === "disconnected") return "切断されました";
    return "接続待機中...";
  };

  const handleNext = () => {
    sendKey("ENTER", true);
    setTimeout(() => sendKey("ENTER", false), 100);
  };

  return (
    <div className="viewer-container">
      <style>{`
        .viewer-container {
          font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
          background: #1a1a1a;
          color: #e0e0e0;
          display: flex;
          flex-direction: column;
          align-items: center;
          padding: 20px;
          min-height: 100vh;
        }
        
        .container {
          max-width: 1200px;
          width: 100%;
        }
        
        h1 {
          text-align: center;
          margin-bottom: 30px;
          color: #fff;
        }
        
        .status {
          background: #2a2a2a;
          padding: 15px;
          border-radius: 8px;
          margin-bottom: 20px;
          text-align: center;
        }
        
        .status.connected {
          background: #2a4a2a;
        }
        
        .status.error {
          background: #4a2a2a;
        }
        
        #video {
          width: 100%;
          max-width: 1280px;
          background: #000;
          border-radius: 8px;
          margin-bottom: 20px;
        }
        
        .controls {
          display: flex;
          flex-wrap: wrap;
          gap: 10px;
          justify-content: center;
          margin-bottom: 20px;
        }
        
        button {
          padding: 12px 24px;
          background: #3a3a3a;
          color: #fff;
          border: none;
          border-radius: 6px;
          cursor: pointer;
          font-size: 16px;
          transition: background 0.2s;
        }
        
        button:hover {
          background: #4a4a4a;
        }
        
        button:active {
          background: #2a2a2a;
        }
        
        button.primary {
          background: #0066cc;
        }
        
        button.primary:hover {
          background: #0055aa;
        }
        
        .log {
          background: #2a2a2a;
          padding: 15px;
          border-radius: 8px;
          max-height: 200px;
          overflow-y: auto;
          font-family: 'Courier New', monospace;
          font-size: 12px;
        }
        
        .info {
          background: #242424;
          padding: 12px;
          border-radius: 8px;
          margin-bottom: 20px;
          font-family: 'Courier New', monospace;
          font-size: 13px;
          white-space: pre-wrap;
          line-height: 1.5;
        }
        
        .log-entry {
          margin-bottom: 5px;
          color: #aaa;
        }
        
        .log-entry.error {
          color: #ff6666;
        }
        
        .log-entry.success {
          color: #66ff66;
        }
        
        .codec-selector {
          background: #2a2a2a;
          padding: 15px;
          border-radius: 8px;
          margin-bottom: 20px;
          display: flex;
          flex-wrap: wrap;
          align-items: center;
          gap: 15px;
        }
        
        .codec-selector label {
          font-size: 14px;
          color: #ccc;
          display: flex;
          align-items: center;
          gap: 8px;
          cursor: pointer;
        }
        
        .codec-selector input[type="radio"] {
          width: 18px;
          height: 18px;
          cursor: pointer;
          accent-color: #0066cc;
        }
        
        .codec-selector .label-text {
          font-weight: 500;
        }
        
        .session-input {
          background: #2a2a2a;
          padding: 15px;
          border-radius: 8px;
          margin-bottom: 20px;
          display: flex;
          flex-wrap: wrap;
          align-items: center;
          gap: 15px;
        }
        
        .session-input input {
          padding: 8px 12px;
          background: #3a3a3a;
          color: #fff;
          border: 1px solid #4a4a4a;
          border-radius: 4px;
          font-size: 14px;
        }
        
        .session-input input:focus {
          outline: none;
          border-color: #0066cc;
        }
      `}</style>

      <div className="container">
        <h1>RemoteRG</h1>

        <div className={`status ${getStatusClass()}`}>{getStatusText()}</div>

        <div className="session-input">
          <label className="label-text">セッションID:</label>
          <input
            type="text"
            value={sessionId}
            onChange={(e) => setSessionId(e.target.value)}
            placeholder="fixed"
            disabled={
              connectionState === "connecting" ||
              connectionState === "connected"
            }
          />
        </div>

        <div className="codec-selector">
          <span className="label-text">コーデック:</span>
          <label>
            <input
              type="radio"
              name="codec"
              value="h264"
              checked={codec === "h264"}
              onChange={() => setCodec("h264")}
              disabled={
                connectionState === "connecting" ||
                connectionState === "connected"
              }
            />
            <span>H.264</span>
          </label>
          <label>
            <input
              type="radio"
              name="codec"
              value="any"
              checked={codec === "any"}
              onChange={() => setCodec("any")}
              disabled={
                connectionState === "connecting" ||
                connectionState === "connected"
              }
            />
            <span>自動</span>
          </label>
        </div>

        <div className="info">
          {stats.inbound && (
            <>
              inbound: bytes={stats.inbound.bytesReceived ?? "-"} frames=
              {stats.inbound.framesReceived ?? "-"} packetsLost=
              {stats.inbound.packetsLost ?? "-"}
              {"\n"}
            </>
          )}
          {stats.track && (
            <>
              track stats: decoded={stats.track.framesDecoded ?? "-"} dropped=
              {stats.track.framesDropped ?? "-"} freeze=
              {stats.track.freezeCount ?? "-"}
              {"\n"}
            </>
          )}
          connectionState: {connectionState}
          {"\n"}
          iceConnectionState: {iceConnectionState}
        </div>

        <video
          id="video"
          ref={videoRef}
          autoPlay
          playsInline
          muted
          style={{
            width: "100%",
            maxWidth: "1280px",
            background: "#000",
            borderRadius: "8px",
            marginBottom: "20px",
          }}
        />

        <div className="controls">
          {connectionState === "disconnected" ||
          connectionState === "error" ||
          connectionState === "failed" ? (
            <button className="primary" onClick={connect}>
              接続
            </button>
          ) : (
            <button onClick={disconnect}>切断</button>
          )}
          <button onClick={handleNext}>次へ (Enter)</button>
          <button onClick={requestScreenshot}>スクリーンショット</button>
        </div>

        <div className="log">
          {logs.map((log, index) => (
            <div key={index} className={`log-entry ${log.type}`}>
              [{log.time}] {log.message}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

