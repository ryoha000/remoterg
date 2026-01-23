import { createFileRoute } from "@tanstack/react-router";
import { useState, useRef, useEffect, useCallback } from "react";
import { useWebRTC } from "@/lib/use-webrtc";
import { Button } from "@/components/ui/button";

import { ViewerOverlay } from "@/components/viewer/viewer-overlay";
import { GalleryModal, type GalleryImage } from "@/components/viewer/gallery-modal";
import { SettingsModal } from "@/components/viewer/settings-modal";
import { type LlmConfig } from "@/lib/webrtc/data-channel";
import { parse } from "best-effort-json-parser";

import * as v from "valibot";

const CodecSchema = v.picklist(["h264", "any"]);

export const Route = createFileRoute("/viewer/$sessionId/$codec")({
  component: ViewerPage,
});

// ... (skip lines)
function ViewerPage() {
  const { sessionId, codec: rawCodec } = Route.useParams();
  const codec = v.parse(CodecSchema, rawCodec);
  const [stream, setStream] = useState<MediaStream | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const [isMuted, setIsMuted] = useState(false); // Default to unmuted, browsers usually require interaction anyway
  const [volume, setVolume] = useState(100);
  const [showOverlay, setShowOverlay] = useState(true);
  const [showDebug, setShowDebug] = useState(false);
  const overlayTimerRef = useRef<number | null>(null);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [galleryOpen, setGalleryOpen] = useState(false);

  const [galleryImages, setGalleryImages] = useState<GalleryImage[]>([]);
  const [llmConfig, setLlmConfig] = useState<LlmConfig | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const handleTrack = useCallback((receivedStream: MediaStream) => {
    setStream(receivedStream);
  }, []);

  const handleScreenshot = useCallback(
    (blob: Blob, meta: { id: string; format: string; size: number }) => {
      const url = URL.createObjectURL(blob);
      const date = new Date();

      // Add to gallery
      setGalleryImages((prev) => [
        ...prev,
        { id: meta.id, url, date, format: meta.format, size: meta.size },
      ]);

      // Auto-download (preserve existing behavior)
      const a = document.createElement("a");
      a.href = url;
      a.download = `screenshot-${meta.id}.${meta.format}`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    },
    [],
  );

  const handleAnalyzeResult = useCallback((id: string, text: string) => {
    console.log(`Analyze result for ${id}:`, text);
    try {
      const analysis = JSON.parse(text);
      setGalleryImages((prev) =>
        prev.map((img) =>
          img.id === id ? { ...img, analysis, isAnalyzing: false } : img
        )
      );
    } catch (e) {
      console.error("Failed to parse analysis result", e);
    }
  }, []);

  const handleAnalyzeResultDelta = useCallback((id: string, delta: string) => {
    setGalleryImages((prev) =>
      prev.map((img) => {
        if (img.id !== id) return img;

        const newRawText = (img.rawAnalysisText || "") + delta;
        let analysis = img.analysis;
        try {
          analysis = parse(newRawText);
        } catch {
          // ignore transient parse errors
        }

        return {
          ...img,
          rawAnalysisText: newRawText,
          analysis,
          isAnalyzing: true,
        };
      })
    );
  }, []);

  const handleAnalyzeDone = useCallback((id: string) => {
    setGalleryImages((prev) =>
      prev.map((img) => (img.id === id ? { ...img, isAnalyzing: false } : img))
    );
  }, []);

  const handleLlmConfig = useCallback((config: LlmConfig) => {
    console.log("LLM Config received:", config);
    setLlmConfig(config);
  }, []);

  const {
    connectionState,
    iceConnectionState,
    stats,
    connect,
    disconnect,
    logs,
    requestScreenshot,
    requestAnalyze,

    requestGetLlmConfig,
    requestUpdateLlmConfig,
    simulateWsClose,
    simulatePcClose,
    sendMouseClick,
  } = useWebRTC({
    signalUrl: "/api/signal",
    sessionId,
    codec,
    onTrack: handleTrack,
    onScreenshot: handleScreenshot,
    onAnalyzeResult: handleAnalyzeResult,
    onAnalyzeResultDelta: handleAnalyzeResultDelta,
    onAnalyzeDone: handleAnalyzeDone,
    onLlmConfig: handleLlmConfig,
  });

  // Auto-connect on mount
  useEffect(() => {
    connect();
    return () => {
      disconnect();
    };
  }, [connect, disconnect]);

  useEffect(() => {
    const videoEl = videoRef.current;
    if (stream && videoEl) {
      // Only assign if different to strictly avoid resetting playback
      if (videoEl.srcObject !== stream) {
        videoEl.srcObject = stream;
        videoEl.play().catch((e) => {
          if (e.name !== "AbortError") {
            console.error("Video playback error:", e);
          }
        });
      }
    }
  }, [stream]);

  // Sync volume/mute with video element
  useEffect(() => {
    if (videoRef.current) {
      videoRef.current.volume = volume / 100;
      videoRef.current.muted = isMuted;
    }
  }, [volume, isMuted]);

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
      void document.documentElement.requestFullscreen();
      setIsFullscreen(true);
    } else {
      void document.exitFullscreen();
      setIsFullscreen(false);
    }
  };

  const handleVideoClick = (e: React.MouseEvent<HTMLVideoElement>) => {
    const video = videoRef.current;
    if (!video) return;

    const rect = video.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    // Calculate displayed video area (letterboxing)
    const videoRatio = video.videoWidth / video.videoHeight;
    const elementRatio = rect.width / rect.height;

    let drawWidth = rect.width;
    let drawHeight = rect.height;
    let startX = 0;
    let startY = 0;

    if (elementRatio > videoRatio) {
      // Pillarbox (black bars on sides)
      drawWidth = rect.height * videoRatio;
      startX = (rect.width - drawWidth) / 2;
    } else {
      // Letterbox (black bars on top/bottom)
      drawHeight = rect.width / videoRatio;
      startY = (rect.height - drawHeight) / 2;
    }

    // specific relative coords
    const relativeX = x - startX;
    const relativeY = y - startY;

    if (relativeX >= 0 && relativeX <= drawWidth && relativeY >= 0 && relativeY <= drawHeight) {
      const normalizedX = relativeX / drawWidth;
      const normalizedY = relativeY / drawHeight;
      console.log(`Click: ${normalizedX.toFixed(3)}, ${normalizedY.toFixed(3)}`);
      sendMouseClick(normalizedX, normalizedY, "left");
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
        className="absolute inset-0 w-full h-full object-contain z-0 cursor-crosshair"
        /* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions */
        onClick={handleVideoClick}
      >
        <track kind="captions" />
      </video>

      {/* Disconnected State */}
      {connectionState !== "connected" && connectionState !== "connecting" && (
        <div className="z-20 flex flex-col items-center justify-center space-y-4 bg-black/80 inset-0 absolute backdrop-blur-sm">
          <h2 className="text-2xl font-bold text-white">Disconnected</h2>
          <Button onClick={connect} variant="secondary">
            Reconnect
          </Button>
        </div>
      )}

      <ViewerOverlay
        show={showOverlay}
        connectionState={connectionState}
        iceConnectionState={iceConnectionState}
        stats={stats}
        logs={logs}
        isMuted={isMuted}
        volume={volume}
        onMuteToggle={() => setIsMuted(!isMuted)}
        onVolumeChange={(v) => {
          setVolume(v);
          if (v > 0) setIsMuted(false);
        }}
        isFullscreen={isFullscreen}
        onToggleFullscreen={toggleFullscreen}
        onBack={() => window.history.back()}
        onDisconnect={disconnect}
        onRequestScreenshot={requestScreenshot}
        onOpenGallery={() => setGalleryOpen(true)}
        onOpenSettings={() => {
          setSettingsOpen(true);
          requestGetLlmConfig();
        }}
        showDebug={showDebug}
        onToggleDebug={setShowDebug}
        onSimulateWsClose={simulateWsClose}
        onSimulatePcClose={simulatePcClose}
      />

      <GalleryModal
        open={galleryOpen}
        onOpenChange={setGalleryOpen}
        images={galleryImages}
        onRequestAnalyze={(id) => {
          requestAnalyze(id);
          setGalleryImages((prev) =>
            prev.map((img) =>
              img.id === id ? { ...img, isAnalyzing: true } : img
            )
          );
        }}
      />

      <SettingsModal
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        config={llmConfig}
        onSave={(newConfig) => {
          console.log("Saving new config:", newConfig);
          requestUpdateLlmConfig(newConfig);
        }}
      />
    </div>
  );
}
