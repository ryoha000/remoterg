import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Download, ArrowLeft, Calendar, FileType, HardDrive, X } from "lucide-react";
import { useState } from "react";

export interface GalleryImage {
  id: string;
  url: string;
  date: Date;
  format: string;
  size: number;
}

interface GalleryModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  images: GalleryImage[];
}

// Format bytes to human readable string
const formatSize = (bytes: number) => {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
};

const handleDownload = (img: GalleryImage) => {
  const a = document.createElement("a");
  a.href = img.url;
  a.download = `screenshot-${img.id}.${img.format}`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
};

export function GalleryModal({ open, onOpenChange, images }: GalleryModalProps) {
  const [selectedImage, setSelectedImage] = useState<GalleryImage | null>(null);

  return (
    <Dialog
      open={open}
      onOpenChange={(val) => {
        onOpenChange(val);
        if (!val) setTimeout(() => setSelectedImage(null), 300); // Reset after close animation
      }}
    >
      <DialogContent
        showCloseButton={false}
        fullScreen
        className="max-w-[95vw] w-full h-[90vh] bg-zinc-950 border-zinc-800 text-zinc-100 flex flex-col p-0 gap-0 overflow-hidden"
      >
        <DialogHeader className="p-4 border-b border-zinc-900 bg-zinc-950/50 backdrop-blur-xl z-10 shrink-0">
          <div className="flex items-center justify-between min-h-8">
            <div className="flex items-center gap-2">
              {selectedImage && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 -ml-2 text-zinc-400 hover:text-white"
                  onClick={() => setSelectedImage(null)}
                >
                  <ArrowLeft className="w-4 h-4" />
                </Button>
              )}
              <DialogTitle>
                {selectedImage ? "Screenshot Details" : "Screenshot Gallery"}
              </DialogTitle>
            </div>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 text-zinc-400 hover:text-white"
              onClick={() => onOpenChange(false)}
            >
              <X className="w-4 h-4" />
              <span className="sr-only">Close</span>
            </Button>
          </div>
          <DialogDescription className="sr-only">
            {selectedImage
              ? `Details for screenshot ${selectedImage.id}`
              : "A gallery of screenshots taken during this session"}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 min-h-0 relative">
          {selectedImage ? (
            // Detail View
            <div className="h-full flex flex-col md:flex-row">
              {/* Image Area */}
              <div className="flex-1 bg-zinc-900/30 flex items-center justify-center p-4 min-h-0 overflow-hidden relative group">
                {/* Checkerboard pattern for transparency */}
                <div
                  className="absolute inset-0 opacity-5"
                  style={{
                    backgroundImage: "radial-gradient(#444 1px, transparent 1px)",
                    backgroundSize: "16px 16px",
                  }}
                />

                <img
                  src={selectedImage.url}
                  alt={selectedImage.id}
                  className="max-w-full max-h-full object-contain shadow-2xl rounded-sm"
                />
              </div>

              {/* Sidebar Info */}
              <div className="w-full md:w-80 bg-zinc-900/50 border-t md:border-t-0 md:border-l border-zinc-800 p-6 flex flex-col gap-6 overflow-y-auto shrink-0">
                <div className="space-y-4">
                  <h3 className="text-lg font-semibold text-white">Metadata</h3>

                  <div className="grid gap-4">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2 text-xs text-zinc-500 uppercase tracking-wider font-medium">
                        <Calendar className="w-3 h-3" />
                        Timestamp
                      </div>
                      <div className="text-sm font-mono text-zinc-300">
                        {selectedImage.date.toLocaleTimeString()}
                        <span className="text-zinc-600 ml-2 text-xs">
                          {selectedImage.date.toLocaleDateString()}
                        </span>
                      </div>
                    </div>

                    <div className="space-y-1">
                      <div className="flex items-center gap-2 text-xs text-zinc-500 uppercase tracking-wider font-medium">
                        <FileType className="w-3 h-3" />
                        Format
                      </div>
                      <div className="text-sm font-mono text-zinc-300 uppercase">
                        {selectedImage.format}
                      </div>
                    </div>

                    <div className="space-y-1">
                      <div className="flex items-center gap-2 text-xs text-zinc-500 uppercase tracking-wider font-medium">
                        <HardDrive className="w-3 h-3" />
                        Size
                      </div>
                      <div className="text-sm font-mono text-zinc-300">
                        {formatSize(selectedImage.size)}
                      </div>
                    </div>

                    <div className="space-y-1">
                      <div className="flex items-center gap-2 text-xs text-zinc-500 uppercase tracking-wider font-medium">
                        ID
                      </div>
                      <div className="text-xs font-mono text-zinc-500 break-all select-all">
                        {selectedImage.id}
                      </div>
                    </div>
                  </div>
                </div>

                <div className="mt-auto">
                  <Button
                    className="w-full"
                    size="lg"
                    onClick={() => handleDownload(selectedImage)}
                  >
                    <Download className="w-4 h-4 mr-2" />
                    Download File
                  </Button>
                </div>
              </div>
            </div>
          ) : (
            // Gallery Grid
            <div className="h-full overflow-y-auto p-4 md:p-6">
              {images.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-full text-zinc-500 gap-4">
                  <div className="w-16 h-16 rounded-full bg-zinc-900/50 flex items-center justify-center">
                    <Download className="w-8 h-8 opacity-20" />
                  </div>
                  <p>No screenshots taken in this session</p>
                </div>
              ) : (
                <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
                  {images
                    .slice()
                    .reverse()
                    .map((img) => (
                      <button
                        key={img.id}
                        onClick={() => setSelectedImage(img)}
                        className="group relative aspect-video bg-zinc-900 rounded-lg overflow-hidden border border-zinc-800 hover:border-zinc-700 hover:ring-2 hover:ring-blue-500/20 transition-all focus:outline-none focus:ring-2 focus:ring-blue-500"
                      >
                        <img
                          src={img.url}
                          alt={img.id}
                          className="w-full h-full object-cover transition-transform duration-500 group-hover:scale-105"
                        />
                        <div className="absolute inset-0 bg-black/0 group-hover:bg-black/20 transition-colors" />

                        <div className="absolute bottom-0 left-0 right-0 p-3 bg-gradient-to-t from-black/90 via-black/50 to-transparent flex items-end justify-between opacity-100 transition-opacity">
                          <span className="text-xs font-mono text-zinc-300">
                            {img.date.toLocaleTimeString()}
                          </span>
                          <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-white/10 text-white/90 backdrop-blur-sm">
                            {img.format.toUpperCase()}
                          </span>
                        </div>
                      </button>
                    ))}
                </div>
              )}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
