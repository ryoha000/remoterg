import { useState, useEffect } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { type LlmConfig } from "@/lib/webrtc/data-channel";
import { Loader2 } from "lucide-react";

interface SettingsModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  config: LlmConfig | null;
  onSave: (config: LlmConfig) => void;
  isLoading?: boolean;
}

export function SettingsModal({
  open,
  onOpenChange,
  config,
  onSave,
  isLoading = false,
}: SettingsModalProps) {
  const [port, setPort] = useState<number>(8081);
  const [modelPath, setModelPath] = useState<string>("");
  const [mmprojPath, setMmprojPath] = useState<string>("");

  useEffect(() => {
    if (config) {
      setPort(config.port);
      // Normalize path to remove double backslashes for display
      setModelPath((config.model_path || "").replaceAll("\\\\", "\\"));
      setMmprojPath((config.mmproj_path || "").replaceAll("\\\\", "\\"));
    }
  }, [config]);

  const handleSave = () => {
    onSave({
      port,
      model_path: modelPath || null,
      mmproj_path: mmprojPath || null,
    });
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]" showCloseButton>
        <DialogHeader>
          <DialogTitle>LLM Settings</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4 py-4">
          <div className="grid grid-cols-4 items-center gap-4">
            <Label htmlFor="port" className="text-right">
              Port
            </Label>
            <Input
              id="port"
              type="number"
              value={port}
              onChange={(e) => setPort(Number(e.target.value))}
              className="col-span-3"
            />
          </div>
          <div className="grid grid-cols-4 items-center gap-4">
            <Label htmlFor="model-path" className="text-right">
              Model Path
            </Label>
            <Input
              id="model-path"
              value={modelPath}
              onChange={(e) => setModelPath(e.target.value)}
              className="col-span-3"
              placeholder="C:\path\to\model.gguf"
            />
          </div>
          <div className="grid grid-cols-4 items-center gap-4">
            <Label htmlFor="mmproj-path" className="text-right">
              MMProj Path
            </Label>
            <Input
              id="mmproj-path"
              value={mmprojPath}
              onChange={(e) => setMmprojPath(e.target.value)}
              className="col-span-3"
              placeholder="C:\path\to\mmproj.gguf"
            />
          </div>
        </div>
        <DialogFooter>
          {isLoading ? (
            <Button disabled>
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              Saving...
            </Button>
          ) : (
            <Button onClick={handleSave}>Save changes</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
