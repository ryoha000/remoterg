import { createFileRoute } from "@tanstack/react-router";
import { useState, useRef, useEffect, useCallback } from "react";
import { useWebRTC } from "@/lib/use-webrtc";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Settings,
  Maximize,
  Minimize,
  Mic,
  MicOff,
  LogOut,
  Signal,
  ArrowLeftIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/viewer/$sessionId/$codec")({
  component: ViewerPage,
});

function ViewerPage() {
  const { sessionId, codec } = Route.useParams();
  const [stream, setStream] = useState<MediaStream | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const [isMuted, setIsMuted] = useState(true);
  const [showOverlay, setShowOverlay] = useState(true);
  const [showDebug, setShowDebug] = useState(false);
  const overlayTimerRef = useRef<number | null>(null);
  const [isFullscreen, setIsFullscreen] = useState(false);

  const { connectionState, iceConnectionState, stats, connect, disconnect } = useWebRTC({
    signalUrl: "/api/signal",
    sessionId,
    codec: codec as "h264" | "any",
    onTrack: (receivedStream) => {
      setStream(receivedStream);
      if (videoRef.current) {
        videoRef.current.srcObject = receivedStream;
        videoRef.current.play().catch(console.error);
      }
    },
  });

  // Auto-connect on mount
  useEffect(() => {
    connect();
    return () => {
      disconnect();
    };
  }, []);

  useEffect(() => {
    if (stream && videoRef.current) {
      videoRef.current.srcObject = stream;
    }
  }, [stream]);

  // Overlay interaction handler
  const handleInteraction = useCallback(() => {
    setShowOverlay(true);
    if (overlayTimerRef.current) {
      window.clearTimeout(overlayTimerRef.current);
    }
    overlayTimerRef.current = window.setTimeout(() => {
      if (connectionState === "connected") {
        setShowOverlay(false);
      }
    }, 3000);
  }, [connectionState]);

  useEffect(() => {
    window.addEventListener("mousemove", handleInteraction);
    window.addEventListener("touchstart", handleInteraction);
    window.addEventListener("keydown", handleInteraction);
    return () => {
      window.removeEventListener("mousemove", handleInteraction);
      window.removeEventListener("touchstart", handleInteraction);
      window.removeEventListener("keydown", handleInteraction);
    };
  }, [handleInteraction]);

  const toggleFullscreen = () => {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen();
      setIsFullscreen(true);
    } else {
      document.exitFullscreen();
      setIsFullscreen(false);
    }
  };

  const getStatusColor = (state: string) => {
    switch (state) {
      case "connected":
        return "bg-green-500";
      case "connecting":
        return "bg-yellow-500";
      case "disconnected":
        return "bg-zinc-500";
      case "failed":
        return "bg-red-500";
      default:
        return "bg-zinc-500";
    }
  };

  return (
    <div className="relative w-full h-screen bg-black overflow-hidden flex items-center justify-center">
      {/* Video Canvas */}
      <video
        ref={videoRef}
        autoPlay
        muted={isMuted}
        playsInline
        className="absolute inset-0 w-full h-full object-contain z-0"
      />

      {/* Disconnected State */}
      {connectionState !== "connected" && connectionState !== "connecting" && (
        <div className="z-20 flex flex-col items-center justify-center space-y-4 bg-black/80 inset-0 absolute backdrop-blur-sm">
          <h2 className="text-2xl font-bold text-white">Disconnected</h2>
          <Button onClick={connect} variant="secondary">
            Reconnect
          </Button>
        </div>
      )}

      {/* Overlay UI */}
      <div
        className={cn(
          "absolute inset-x-0 top-0 z-10 p-4 transition-all duration-300 bg-gradient-to-b from-black/80 to-transparent",
          showOverlay
            ? "opacity-100 translate-y-0"
            : "opacity-0 -translate-y-4 pointer-events-none",
        )}
      >
        <div className="flex items-center justify-between max-w-7xl mx-auto">
          {/* Left: Status */}
          <div className="flex items-center gap-4">
            <Button
              variant="ghost"
              size="icon"
              className="text-white hover:bg-white/20"
              onClick={() => window.history.back()}
              aria-label="Go back"
            >
              <ArrowLeftIcon className="w-5 h-5" />
            </Button>
            <div className="flex items-center gap-2 px-3 py-1.5 bg-black/40 backdrop-blur-md rounded-full border border-white/10">
              <div className={cn("w-2 h-2 rounded-full", getStatusColor(connectionState))} />
              <span className="text-xs font-medium text-white/90 capitalize">
                {connectionState}
              </span>
            </div>
            {connectionState === "connected" && (
              <div className="flex items-center gap-2 px-3 py-1.5 bg-black/40 backdrop-blur-md rounded-full border border-white/10 text-xs text-white/80 font-mono">
                <Signal className="w-3 h-3" />
                <span>{stats.inbound?.packetsLost ?? 0} loss</span>
              </div>
            )}
          </div>

          {/* Right: Controls */}
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              className="text-white hover:bg-white/20 rounded-full"
              onClick={() => {
                const newMuted = !isMuted;
                if (videoRef.current) videoRef.current.muted = newMuted;
                setIsMuted(newMuted);
              }}
              aria-label={isMuted ? "Unmute" : "Mute"}
            >
              {isMuted ? <MicOff className="w-5 h-5" /> : <Mic className="w-5 h-5" />}
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="text-white hover:bg-white/20 rounded-full"
              onClick={toggleFullscreen}
              aria-label={isFullscreen ? "Exit Fullscreen" : "Enter Fullscreen"}
            >
              {isFullscreen ? <Minimize className="w-5 h-5" /> : <Maximize className="w-5 h-5" />}
            </Button>

            <Popover>
              <PopoverTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="text-white hover:bg-white/20 rounded-full"
                  aria-label="Settings"
                >
                  <Settings className="w-5 h-5" />
                </Button>
              </PopoverTrigger>
              <PopoverContent
                className="w-72 bg-zinc-950/95 border-zinc-800 text-zinc-100 backdrop-blur-xl"
                align="end"
                sideOffset={8}
              >
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <h4 className="font-medium text-sm">Settings</h4>
                    <Badge variant="outline" className="text-[10px] border-zinc-700">
                      v0.1.0
                    </Badge>
                  </div>

                  <div className="space-y-3">
                    <div className="flex items-center justify-between">
                      <Label className="text-xs text-zinc-400">Stats for Nerds</Label>
                      <Switch checked={showDebug} onCheckedChange={setShowDebug} />
                    </div>
                    <div className="flex items-center justify-between">
                      <Label className="text-xs text-zinc-400">Mouse Sensitivity</Label>
                      <span className="text-xs text-zinc-500">1.0</span>
                    </div>
                  </div>

                  <div className="pt-2 border-t border-zinc-800">
                    <Button variant="destructive" size="sm" className="w-full" onClick={disconnect}>
                      <LogOut className="w-4 h-4 mr-2" />
                      Disconnect
                    </Button>
                  </div>
                </div>
              </PopoverContent>
            </Popover>
          </div>
        </div>
      </div>

      {/* Debug Overlay */}
      {showDebug && (
        <div className="absolute top-20 left-4 z-10 p-4 bg-black/60 backdrop-blur-md rounded-lg border border-white/10 text-xs font-mono text-green-400 pointer-events-none select-none max-w-sm">
          <div className="space-y-1">
            <p>ICE: {iceConnectionState}</p>
            <p>Inbound: {(stats.inbound?.bytesReceived ?? 0) / 1024} KB</p>
            <p>Frames: {stats.inbound?.framesReceived}</p>
            <p>Loss: {stats.inbound?.packetsLost}</p>
          </div>
        </div>
      )}
    </div>
  );
}
