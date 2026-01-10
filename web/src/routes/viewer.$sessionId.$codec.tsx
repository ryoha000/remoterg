import { createFileRoute } from "@tanstack/react-router";
import { useState, useRef, useEffect } from "react";
import { useWebRTC } from "@/lib/use-webrtc";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ThemeToggle } from "@/components/theme-toggle";

export const Route = createFileRoute("/viewer/$sessionId/$codec")({
  component: ViewerPage,
});

function ViewerPage() {
  const { sessionId, codec } = Route.useParams();
  const [stream, setStream] = useState<MediaStream | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const [isMuted, setIsMuted] = useState(true);

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
    codec: codec as "h264" | "any",
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

  const getStatusText = () => {
    if (connectionState === "connecting") return "接続中...";
    if (connectionState === "connected") return "接続済み";
    if (connectionState === "error") return "接続エラー";
    if (connectionState === "failed") return "接続失敗";
    if (connectionState === "disconnected") return "切断されました";
    return "接続待機中...";
  };

  const getStatusVariant = ():
    | "default"
    | "destructive"
    | "secondary"
    | "outline" => {
    if (connectionState === "connected") return "default";
    if (connectionState === "error" || connectionState === "failed")
      return "destructive";
    return "secondary";
  };

  const handleNext = () => {
    sendKey("ENTER", true);
    setTimeout(() => sendKey("ENTER", false), 100);
  };

  return (
    <div className="min-h-screen bg-background flex flex-col items-center p-6">
      <div className="max-w-6xl w-full space-y-6">
        <div className="flex items-center justify-between">
          <h1 className="text-3xl font-bold text-foreground">RemoteRG</h1>
          <ThemeToggle />
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-center">
              <Badge variant={getStatusVariant()}>{getStatusText()}</Badge>
            </CardTitle>
          </CardHeader>
        </Card>

        <Card>
          <CardContent className="pt-6">
            <div className="flex flex-wrap items-center gap-4">
              <span className="text-sm text-muted-foreground">
                セッションID: {sessionId}
              </span>
              <span className="text-sm text-muted-foreground">
                コーデック: {codec}
              </span>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent className="pt-6">
            <pre className="text-sm font-mono whitespace-pre-wrap text-foreground">
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
                  track stats: decoded={stats.track.framesDecoded ?? "-"}{" "}
                  dropped=
                  {stats.track.framesDropped ?? "-"} freeze=
                  {stats.track.freezeCount ?? "-"}
                  {"\n"}
                </>
              )}
              connectionState: {connectionState}
              {"\n"}
              iceConnectionState: {iceConnectionState}
            </pre>
          </CardContent>
        </Card>

        <video
          ref={videoRef}
          autoPlay
          muted={isMuted}
          playsInline
          className="w-full max-w-5xl bg-black rounded-lg"
        />

        <div className="flex flex-wrap gap-3 justify-center">
          {connectionState === "disconnected" ||
          connectionState === "error" ||
          connectionState === "failed" ? (
            <Button onClick={connect}>接続</Button>
          ) : (
            <Button variant="outline" onClick={disconnect}>
              切断
            </Button>
          )}
          <Button variant="outline" onClick={handleNext}>
            次へ (Enter)
          </Button>
          <Button
            variant="outline"
            onClick={() => {
              if (videoRef.current) {
                const newMuted = !isMuted;
                videoRef.current.muted = newMuted;
                setIsMuted(newMuted);
              }
            }}
          >
            {isMuted ? "🔇 音声ON" : "🔊 音声OFF"}
          </Button>
          <Button variant="outline" onClick={requestScreenshot}>
            スクリーンショット
          </Button>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>ログ</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="max-h-[200px] overflow-y-auto space-y-1 font-mono text-sm">
              {logs.map((log, index) => (
                <div
                  key={index}
                  className={
                    log.type === "error"
                      ? "text-destructive"
                      : log.type === "success"
                      ? "text-green-500"
                      : "text-muted-foreground"
                  }
                >
                  [{log.time}] {log.message}
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
