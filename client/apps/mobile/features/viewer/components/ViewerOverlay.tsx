import { Ionicons } from "@expo/vector-icons"
import { useState } from "react"
import { View, StyleSheet, TouchableOpacity } from "react-native"
import Animated, {
  FadeIn,
  FadeOut,
  Layout,
  useAnimatedStyle,
  withTiming,
} from "react-native-reanimated"
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { Text } from "@/components/ui/text"
import { cn } from "@/lib/utils"

import { GalleryModal } from "../../gallery/components/GalleryModal"
import { AnalysisResult } from "../../gallery/types"

interface ViewerOverlayProps {
  visible: boolean
  status: string
  onDisconnect: () => void
  sessionId: string
  stats: {
    fps: number
    bitrate: number
    loss: number
  }
  onInteraction?: () => void
  onRequestScreenshot?: () => void
  onRequestAnalyze?: (id: string, max_edge?: number) => void
  analysisResults?: Record<string, AnalysisResult>
  isAnalyzingMap?: Record<string, boolean>
}

export function ViewerOverlay({
  visible,
  status,
  onDisconnect,
  sessionId,
  stats,
  onInteraction,
  onRequestScreenshot,
  onRequestAnalyze,
  analysisResults,
  isAnalyzingMap,
}: ViewerOverlayProps) {
  const [showSettings, setShowSettings] = useState(false)
  const [showDebug, setShowDebug] = useState(false)
  const [showGallery, setShowGallery] = useState(false)
  const insets = useSafeAreaInsets()

  const handleInteraction = () => {
    onInteraction?.()
  }

  const panelStyle = useAnimatedStyle(() => {
    return {
      top: withTiming(visible ? 90 : insets.top + 20, { duration: 200 }),
    }
  })

  return (
    <View style={StyleSheet.absoluteFill} pointerEvents="box-none">
      {/* Top Bar */}
      {visible && (
        <Animated.View
          entering={FadeIn.duration(200)}
          exiting={FadeOut.duration(200)}
          style={styles.topBar}
          onStartShouldSetResponder={() => true}
          onTouchStart={handleInteraction}
        >
          <SafeAreaView edges={["top", "left", "right"]} style={styles.safeArea}>
            <View className="flex-row justify-between items-center px-4 pb-2">
              <View className="flex-row items-center gap-4">
                <Button
                  variant="ghost"
                  size="icon"
                  onPress={onDisconnect}
                  className="rounded-full active:bg-white/20"
                >
                  <Ionicons name="arrow-back" size={24} color="white" />
                </Button>
                <View className="flex-row items-center gap-2 px-3 py-1.5 bg-black/40 rounded-full border border-white/10">
                  <View
                    className={cn(
                      "w-2 h-2 rounded-full",
                      status.includes("connected") ? "bg-green-500" : "bg-yellow-500",
                    )}
                  />
                  <Text className="text-white/90 text-xs font-medium capitalize">{status}</Text>
                </View>
                {status.includes("connected") && (
                  <View className="flex-row items-center gap-2 px-3 py-1.5 bg-black/40 rounded-full border border-white/10">
                    <Ionicons name="cellular" size={12} color="rgba(255,255,255,0.8)" />
                    <Text className="text-white/80 text-xs font-mono">{stats.loss}% loss</Text>
                  </View>
                )}
              </View>

              {/* Right side settings/debug toggles */}
              <View className="flex-row items-center gap-2">
                <Button
                  variant="ghost"
                  size="icon"
                  onPress={() => setShowDebug(!showDebug)}
                  className={cn("rounded-full active:bg-white/20", showDebug && "bg-white/20")}
                >
                  <Ionicons name="bug-outline" size={20} color="white" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  onPress={() => setShowGallery(true)}
                  className="rounded-full active:bg-white/20"
                >
                  <Ionicons name="images-outline" size={20} color="white" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  onPress={onRequestScreenshot}
                  className="rounded-full active:bg-white/20"
                >
                  <Ionicons name="camera-outline" size={20} color="white" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  onPress={() => setShowSettings(!showSettings)}
                  className={cn("rounded-full active:bg-white/20", showSettings && "bg-white/20")}
                >
                  <Ionicons name="settings-outline" size={20} color="white" />
                </Button>
              </View>
            </View>
          </SafeAreaView>
        </Animated.View>
      )}

      {/* Settings Backdrop */}
      {showSettings && (
        <TouchableOpacity
          style={[StyleSheet.absoluteFill, { zIndex: 55 }]}
          activeOpacity={1}
          onPress={() => setShowSettings(false)}
        >
          <View style={{ flex: 1 }} />
        </TouchableOpacity>
      )}

      {/* Settings Panel (Overlay) */}
      {showSettings && (
        <Animated.View
          entering={FadeIn.duration(200)}
          exiting={FadeOut.duration(200)}
          style={[styles.settingsPanel, panelStyle]}
          onStartShouldSetResponder={() => true}
          onTouchStart={handleInteraction}
        >
          <View className="bg-zinc-900/95 p-4 rounded-xl border border-zinc-800 w-64 backdrop-blur-xl">
            <View className="flex-row justify-between items-center mb-4">
              <Text className="text-white font-medium text-sm">Settings</Text>
              <Text className="text-zinc-500 text-[10px]">v0.1.0</Text>
            </View>

            {/* Audio (Mock) */}
            <View className="mb-4">
              <Text className="text-zinc-400 text-xs mb-2">Audio</Text>
              <View className="flex-row items-center gap-2">
                <Ionicons name="volume-medium" size={16} color="#a1a1aa" />
                <View className="h-1 flex-1 bg-zinc-700 rounded-full" />
                <Text className="text-zinc-500 text-xs">50%</Text>
              </View>
            </View>

            <Button variant="destructive" size="sm" className="w-full" onPress={onDisconnect}>
              <View className="flex-row items-center gap-2">
                <Ionicons name="log-out-outline" size={16} color="white" />
                <Text className="text-white text-xs">Disconnect</Text>
              </View>
            </Button>
          </View>
        </Animated.View>
      )}

      {/* Debug Panel */}
      {showDebug && (
        <Animated.View
          entering={FadeIn.duration(200)}
          exiting={FadeOut.duration(200)}
          style={[styles.debugPanel, panelStyle]}
          onStartShouldSetResponder={() => true}
          onTouchStart={handleInteraction}
        >
          <View className="bg-black/60 p-3 rounded-lg border border-white/10 backdrop-blur-md">
            <Text className="text-green-400 font-mono text-xs">FPS: {stats.fps}</Text>
            <Text className="text-green-400 font-mono text-xs">Bitrate: {stats.bitrate} kbps</Text>
            <Text className="text-green-400 font-mono text-xs">Loss: {stats.loss}%</Text>
            <Text className="text-green-400 font-mono text-xs mt-1">
              Session: {sessionId.slice(0, 8)}...
            </Text>
          </View>
        </Animated.View>
      )}
      <GalleryModal
        visible={showGallery}
        onClose={() => setShowGallery(false)}
        onRequestAnalyze={onRequestAnalyze}
        analysisResults={analysisResults}
        isAnalyzingMap={isAnalyzingMap}
      />
    </View>
  )
}

const styles = StyleSheet.create({
  topBar: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    zIndex: 50,
    backgroundColor: "rgba(0,0,0,0.4)", // Gradient replacement since RN doesn't have linear-gradient built-in easily without lib
  },
  safeArea: {
    width: "100%",
  },
  bottomBar: {
    position: "absolute",
    bottom: 0,
    left: 0,
    right: 0,
    zIndex: 40,
    alignItems: "flex-end",
  },
  settingsPanel: {
    position: "absolute",
    // top: 100, // Handled dynamically
    right: 16,
    zIndex: 60,
  },
  debugPanel: {
    position: "absolute",
    // top: 100, // Handled dynamically
    left: 16,
    zIndex: 60,
  },
})
