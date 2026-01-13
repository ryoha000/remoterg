import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useEffect } from "react";
import { ArrowRight, Monitor, Settings2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ThemeToggle } from "@/components/theme-toggle";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";

export const Route = createFileRoute("/")({ component: App });

function App() {
  const [sessionId, setSessionId] = useState<string>("fixed");
  const [isH264, setIsH264] = useState(true);
  const [mounted, setMounted] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    setMounted(true);
  }, []);

  const handleConnect = () => {
    navigate({
      to: "/viewer/$sessionId/$codec",
      params: {
        sessionId: sessionId || "fixed",
        codec: isH264 ? "h264" : "any",
      },
    });
  };

  return (
    <div className="min-h-screen w-full flex flex-col items-center justify-center relative p-4 transition-colors duration-300">
      <div className="absolute top-6 right-6 flex items-center gap-2 animate-in fade-in slide-in-from-top-4 duration-700">
        <ThemeToggle />
      </div>

      <div
        className={`w-full max-w-sm space-y-8 transition-opacity duration-1000 ${mounted ? "opacity-100" : "opacity-0"}`}
      >
        {/* Header */}
        <div className="text-center space-y-2">
          <div className="inline-flex items-center justify-center p-3 bg-primary/5 rounded-2xl mb-4">
            <Monitor className="w-8 h-8 text-primary" />
          </div>
          <h1 className="text-3xl font-bold tracking-tight">RemoteRG</h1>
          <p className="text-muted-foreground text-sm">High-performance remote gaming</p>
        </div>

        {/* Status Indicator */}
        <div className="flex justify-center">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-secondary text-secondary-foreground text-xs font-medium">
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-500 opacity-75"></span>
              <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"></span>
            </span>
            System Operational
          </div>
        </div>

        {/* Main Actions */}
        <div className="space-y-4 pt-4">
          <Button
            size="lg"
            className="w-full text-base h-12 rounded-xl transition-all hover:scale-[1.02] active:scale-[0.98]"
            onClick={handleConnect}
          >
            Connect
            <ArrowRight className="w-4 h-4 ml-2" />
          </Button>

          {/* Advanced Settings Popover */}
          <div className="flex justify-center">
            <Popover>
              <PopoverTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-muted-foreground hover:text-foreground transition-colors"
                >
                  <Settings2 className="w-4 h-4 mr-2" />
                  Connection Settings
                </Button>
              </PopoverTrigger>
              <PopoverContent className="w-80 p-4" align="center">
                <div className="space-y-4">
                  <h4 className="font-medium leading-none">Settings</h4>
                  <div className="grid gap-2">
                    <div className="grid grid-cols-3 items-center gap-4">
                      <Label htmlFor="session">Session ID</Label>
                      <Input
                        id="session"
                        value={sessionId}
                        onChange={(e) => setSessionId(e.target.value)}
                        className="col-span-2 h-8"
                      />
                    </div>
                    <div className="flex items-center justify-between">
                      <Label htmlFor="codec-h264">Force H.264</Label>
                      <Switch id="codec-h264" checked={isH264} onCheckedChange={setIsH264} />
                    </div>
                  </div>
                </div>
              </PopoverContent>
            </Popover>
          </div>
        </div>
      </div>

      {/* Footer */}
      <div className="absolute bottom-6 text-center">
        <p className="text-[10px] text-muted-foreground/40 uppercase tracking-widest font-mono">
          v0.1.0 • Stable
        </p>
      </div>
    </div>
  );
}
