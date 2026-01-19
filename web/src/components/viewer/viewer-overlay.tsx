import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import {
  Settings,
  Maximize,
  Minimize,
  LogOut,
  Signal,
  ArrowLeftIcon,
  Volume2,
  VolumeX,
  Bug,
  Camera,
  Images,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { WebRTCStats } from "@/lib/use-webrtc";

export interface LogEntry {
  time: string;
  message: string;
  type: string;
}

interface ViewerOverlayProps {
  show: boolean;
  connectionState: string;
  iceConnectionState: string;
  stats: WebRTCStats;
  logs: LogEntry[];

  // Audio
  isMuted: boolean;
  volume: number;
  onMuteToggle: () => void;
  onVolumeChange: (val: number) => void;

  // View
  isFullscreen: boolean;
  onToggleFullscreen: () => void;

  // Actions
  onBack: () => void;
  onDisconnect: () => void;
  onRequestScreenshot: () => void;
  onOpenGallery: () => void;
  onOpenSettings: () => void;

  // Debug
  showDebug: boolean;
  onToggleDebug: (show: boolean) => void;
  onSimulateWsClose: () => void;
  onSimulatePcClose: () => void;
}

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

export function ViewerOverlay({
  show,
  connectionState,
  iceConnectionState,
  stats,
  logs,
  isMuted,
  volume,
  onMuteToggle,
  onVolumeChange,
  isFullscreen,
  onToggleFullscreen,
  onBack,
  onDisconnect,
  onRequestScreenshot,
  onOpenGallery,
  onOpenSettings,
  showDebug,
  onToggleDebug,
  onSimulateWsClose,
  onSimulatePcClose,
}: ViewerOverlayProps) {
  return (
    <>
      {/* Top Bar Overlay */}
      <div
        className={cn(
          "absolute inset-x-0 top-0 z-10 p-4 transition-all duration-300 bg-gradient-to-b from-black/80 to-transparent",
          show ? "opacity-100 translate-y-0" : "opacity-0 -translate-y-4 pointer-events-none",
        )}
      >
        <div className="flex items-center justify-between max-w-7xl mx-auto">
          {/* Left: Status */}
          <div className="flex items-center gap-4">
            <Button
              variant="ghost"
              size="icon"
              className="text-white hover:bg-white/20"
              onClick={onBack}
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
              onClick={onToggleFullscreen}
              aria-label={isFullscreen ? "Exit Fullscreen" : "Enter Fullscreen"}
            >
              {isFullscreen ? <Minimize className="w-5 h-5" /> : <Maximize className="w-5 h-5" />}
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="text-white hover:bg-white/20 rounded-full"
              onClick={onRequestScreenshot}
              aria-label="Take Screenshot"
            >
              <Camera className="w-5 h-5" />
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="text-white hover:bg-white/20 rounded-full"
              onClick={onOpenGallery}
              aria-label="Open Gallery"
            >
              <Images className="w-5 h-5" />
            </Button>

            <Popover>
              <PopoverTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="text-white hover:bg-white/20 rounded-full"
                  aria-label="Debug"
                >
                  <Bug className="w-5 h-5" />
                </Button>
              </PopoverTrigger>
              <PopoverContent
                className="w-96 bg-zinc-950/95 border-zinc-800 text-zinc-100 backdrop-blur-xl"
                align="end"
                sideOffset={8}
              >
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <h4 className="font-medium text-sm">Debug Tools</h4>
                    <Badge variant="outline" className="text-[10px] border-zinc-700">
                      Dev
                    </Badge>
                  </div>

                  <div className="space-y-4">
                    {/* Connection Stats */}
                    <div className="space-y-2 p-3 bg-zinc-900/50 rounded-lg border border-zinc-800">
                      <div className="grid grid-cols-2 gap-2 text-xs">
                        <span className="text-zinc-500">Connection State</span>
                        <span className="font-mono text-right">{connectionState}</span>
                        <span className="text-zinc-500">ICE State</span>
                        <span className="font-mono text-right">{iceConnectionState}</span>
                      </div>
                    </div>

                    {/* Simulation Actions */}
                    <div className="space-y-2">
                      <Label className="text-xs text-zinc-400">Simulation</Label>
                      <div className="grid grid-cols-2 gap-2">
                        <Button
                          variant="destructive"
                          size="sm"
                          className="w-full text-xs"
                          onClick={onSimulateWsClose}
                        >
                          Simulate WS Error
                        </Button>
                        <Button
                          variant="destructive"
                          size="sm"
                          className="w-full text-xs"
                          onClick={onSimulatePcClose}
                        >
                          Simulate PC Error
                        </Button>
                      </div>
                    </div>

                    {/* Quick Info */}
                    <div className="flex items-center justify-between">
                      <Label className="text-xs text-zinc-400">Stats Overlay</Label>
                      <Switch checked={showDebug} onCheckedChange={onToggleDebug} />
                    </div>

                    {/* Logs */}
                    <div className="space-y-2">
                      <Label className="text-xs text-zinc-400">Logs ({logs.length})</Label>
                      <div className="h-48 overflow-y-auto rounded-md bg-black/50 p-2 text-[10px] font-mono border border-zinc-800">
                        {logs
                          .slice()
                          .reverse()
                          .map((log, i) => (
                            <div key={`${i}-${log.time}`} className="mb-1">
                              <span className="text-zinc-500">[{log.time}]</span>{" "}
                              <span
                                className={cn(
                                  log.type === "error"
                                    ? "text-red-400"
                                    : log.type === "success"
                                      ? "text-green-400"
                                      : "text-zinc-300",
                                )}
                              >
                                {log.message}
                              </span>
                            </div>
                          ))}
                      </div>
                    </div>
                  </div>
                </div>
              </PopoverContent>
            </Popover>

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

                  <div className="space-y-4">
                    {/* Audio Settings */}
                    <div className="space-y-3">
                      <Label className="text-xs text-zinc-400">Audio</Label>
                      <div className="flex items-center gap-3">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-zinc-400 hover:text-white"
                          onClick={onMuteToggle}
                        >
                          {isMuted || volume === 0 ? (
                            <VolumeX className="w-4 h-4" />
                          ) : (
                            <Volume2 className="w-4 h-4" />
                          )}
                        </Button>
                        <Slider
                          value={[isMuted ? 0 : volume]}
                          max={100}
                          step={1}
                          className="flex-1"
                          onValueChange={(vals) => {
                            const val = vals[0] ?? 0;
                            onVolumeChange(val);
                          }}
                        />
                        <span className="text-xs text-zinc-500 w-8 text-right">
                          {isMuted ? 0 : volume}%
                        </span>
                      </div>
                    </div>

                    <div className="flex items-center justify-between">
                      <Label className="text-xs text-zinc-400">Mouse Sensitivity</Label>
                      <span className="text-xs text-zinc-500">1.0</span>
                    </div>

                    <Button
                      variant="secondary"
                      size="sm"
                      className="w-full"
                      onClick={onOpenSettings}
                    >
                      <Settings className="w-4 h-4 mr-2" />
                      LLM Config
                    </Button>
                  </div>

                  <div className="pt-2 border-t border-zinc-800">
                    <Button
                      variant="destructive"
                      size="sm"
                      className="w-full"
                      onClick={onDisconnect}
                    >
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

      {/* Debug Overlay Box */}
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
    </>
  );
}
