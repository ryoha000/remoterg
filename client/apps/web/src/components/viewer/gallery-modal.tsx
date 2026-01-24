import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Download, ArrowLeft, Calendar, FileType, HardDrive, X, Sparkles, Loader2, User, MapPin, MessageSquare } from "lucide-react";
import { useState, useMemo } from "react";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";

// JSON Schema Types
interface SceneInfo {
  location: string;
  time_of_day: string;
  atmosphere: string;
}

interface Dialogue {
  speaker: string;
  text: string;
}

interface Character {
  name: string;
  expression_tags: string[];
  visual_description: string;
  position: string;
}

export interface AnalysisResult {
  scene_info?: SceneInfo;
  dialogue?: Dialogue;
  characters?: Character[];
}

export interface GalleryImage {
  id: string;
  url: string;
  date: Date;
  format: string;
  size: number;
  isAnalyzing?: boolean;
  rawAnalysisText?: string;
  analysis?: AnalysisResult;
}

interface GalleryModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  images: GalleryImage[];
  onRequestAnalyze: (id: string, max_edge?: number) => void;
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

function AnalysisViewer({ analysis }: { analysis: AnalysisResult }) {
  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-500">
      {/* Scene Info */}
      {analysis.scene_info && (
        <div className="space-y-2">
          <h4 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <MapPin className="w-4 h-4" /> Scene
          </h4>
          <div className="bg-zinc-900/50 rounded-md p-3 text-sm space-y-1 border border-zinc-800">
            <div className="grid grid-cols-[80px_1fr] gap-2">
              <span className="text-zinc-500">Location</span>
              <span className="text-zinc-200">{analysis.scene_info.location}</span>
            </div>
            <div className="grid grid-cols-[80px_1fr] gap-2">
              <span className="text-zinc-500">Time</span>
              <span className="text-zinc-200">{analysis.scene_info.time_of_day}</span>
            </div>
            <div className="grid grid-cols-[80px_1fr] gap-2">
              <span className="text-zinc-500">Mood</span>
              <span className="text-zinc-200">{analysis.scene_info.atmosphere}</span>
            </div>
          </div>
        </div>
      )}

      {/* Dialogue */}
      {analysis.dialogue && (
        <div className="space-y-2">
          <h4 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <MessageSquare className="w-4 h-4" /> Dialogue
          </h4>
          <div className="bg-zinc-900/50 rounded-md p-3 text-sm border border-zinc-800">
             <div className="font-semibold text-indigo-300 mb-1">{analysis.dialogue.speaker}</div>
             <div className="text-zinc-200 whitespace-pre-wrap leading-relaxed">{analysis.dialogue.text}</div>
          </div>
        </div>
      )}

      {/* Characters */}
      {analysis.characters && analysis.characters.length > 0 && (
        <div className="space-y-2">
          <h4 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <User className="w-4 h-4" /> Characters
          </h4>
          <div className="space-y-3">
            {analysis.characters.map((char, i) => (
              <div key={i} className="bg-zinc-900/50 rounded-md p-3 text-sm border border-zinc-800">
                <div className="flex items-center justify-between mb-2">
                  <span className="font-semibold text-emerald-300">{char.name}</span>
                  <span className="text-xs text-zinc-500 uppercase">{char.position}</span>
                </div>
                <div className="text-zinc-300 mb-2">{char.visual_description}</div>
                <div className="flex flex-wrap gap-1">
                  {char.expression_tags?.map((tag, j) => (
                    <Badge key={j} variant="secondary" className="bg-zinc-800 text-zinc-400 text-[10px] hover:bg-zinc-700">
                      {tag}
                    </Badge>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function GalleryModal({ open, onOpenChange, images, onRequestAnalyze }: GalleryModalProps) {
  const [selectedImageId, setSelectedImageId] = useState<string | null>(null);

  const selectedImage = useMemo(() => 
    images.find((img) => img.id === selectedImageId) || null,
  [images, selectedImageId]);

  return (
    <Dialog
      open={open}
      onOpenChange={(val) => {
        onOpenChange(val);
        if (!val) setTimeout(() => setSelectedImageId(null), 300); // Reset after close animation
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
              {selectedImageId && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 -ml-2 text-zinc-400 hover:text-white"
                  onClick={() => setSelectedImageId(null)}
                >
                  <ArrowLeft className="w-4 h-4" />
                </Button>
              )}
              <DialogTitle>
                {selectedImageId ? "Screenshot Details" : "Screenshot Gallery"}
              </DialogTitle>
            </div>
            <div className="flex items-center gap-2">
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
          </div>
          <DialogDescription className="sr-only">
            {selectedImageId
              ? "Details for selected screenshot"
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
              <div className="w-full md:w-96 bg-zinc-900/50 border-t md:border-t-0 md:border-l border-zinc-800 flex flex-col overflow-hidden shrink-0 transition-all min-h-0 max-h-full">
                <ScrollArea className="flex-1 p-6 max-h-full">
                  <div className="space-y-6">
                    {/* Metadata */}
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
                      </div>
                    </div>

                    <div className="h-px bg-zinc-800" />

                    {/* AI Analysis */}
                    <div className="space-y-4">
                      <div className="flex items-center justify-between">
                        <h3 className="text-lg font-semibold text-white flex items-center gap-2">
                          <Sparkles className="w-4 h-4 text-purple-400" />
                          AI Analysis
                        </h3>
                      </div>
                      
                      {selectedImage.analysis ? (
                        <div className="space-y-4">
                          <AnalysisViewer analysis={selectedImage.analysis} />
                          {selectedImage.isAnalyzing && (
                            <div className="flex items-center justify-center py-2 text-xs text-zinc-500 gap-2 animate-pulse">
                              <Loader2 className="w-3 h-3 animate-spin" />
                              Generating...
                            </div>
                          )}
                        </div>
                      ) : selectedImage.isAnalyzing ? (
                        <div className="flex flex-col items-center justify-center py-8 gap-3 text-zinc-500">
                          <Loader2 className="w-8 h-8 animate-spin text-purple-500" />
                          <p className="text-sm animate-pulse">Analyzing image context...</p>
                        </div>
                      ) : (
                        <div className="bg-zinc-900/30 rounded-lg p-4 border border-zinc-800/50 text-center space-y-3">
                          <p className="text-sm text-zinc-400">
                            Get insights about the scene, characters, and dialogue using AI.
                          </p>
                          <Button
                            className="w-full gap-2 border-zinc-700 bg-zinc-800 hover:bg-zinc-700"
                            variant="outline"
                            onClick={() => onRequestAnalyze(selectedImage.id, 512)}
                          >
                            <Sparkles className="w-4 h-4 text-purple-400" />
                            Analyze Screenshot
                          </Button>
                        </div>
                      )}
                    </div>
                  </div>
                </ScrollArea>

                <div className="p-4 border-t border-zinc-800 bg-zinc-900/50">
                  <Button
                    className="w-full"
                    size="lg"
                    onClick={() => handleDownload(selectedImage)}
                  >
                    <Download className="w-4 h-4 mr-2" />
                    Download
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
                        onClick={() => setSelectedImageId(img.id)}
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
                        {img.isAnalyzing && (
                             <div className="absolute top-2 right-2 p-1.5 bg-black/50 rounded-full backdrop-blur-md">
                                <Loader2 className="w-3 h-3 text-purple-400 animate-spin" />
                             </div>
                        )}
                        {img.analysis && (
                             <div className="absolute top-2 right-2 p-1.5 bg-black/50 rounded-full backdrop-blur-md">
                                <Sparkles className="w-3 h-3 text-emerald-400" />
                             </div>
                        )}
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
