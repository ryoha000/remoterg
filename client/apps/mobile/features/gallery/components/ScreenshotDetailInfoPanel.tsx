import { Ionicons } from "@expo/vector-icons"
import * as MediaLibrary from "expo-media-library"
import { View, Text, ActivityIndicator, ScrollView, ViewStyle, StyleProp } from "react-native"
import Animated, { AnimatedStyle } from "react-native-reanimated"
import { SafeAreaView } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { AnalysisResult } from "@/features/gallery/types"

import { AnalysisViewer } from "./AnalysisViewer"

interface ScreenshotDetailInfoPanelProps {
  currentAsset: MediaLibrary.Asset
  currentAssetInfo?: MediaLibrary.AssetInfo
  analysis?: AnalysisResult | null
  isAnalyzing: boolean
  hostId: string | null
  onRequestAnalyze?: (id: string, max_edge?: number) => void
  style?: StyleProp<AnimatedStyle<ViewStyle>>
  dbHostInfo?:
    | {
        hostId: string
        windowTitle: string | null
        processName: string | null
        processPath: string | null
      }
    | undefined
}

export const ScreenshotDetailInfoPanel = ({
  currentAsset,
  currentAssetInfo,
  analysis,
  isAnalyzing,
  hostId,
  onRequestAnalyze,
  style,
  dbHostInfo,
}: ScreenshotDetailInfoPanelProps) => {
  const formatSize = (bytes?: number) => {
    if (!bytes) return "Unknown"
    const k = 1024
    const sizes = ["B", "KB", "MB", "GB"]
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i]
  }

  return (
    <Animated.View
      style={[
        {
          position: "absolute",
          right: 0,
          top: 0,
          bottom: 0,
          backgroundColor: "#18181b", // zinc-900
          borderLeftWidth: 1,
          borderColor: "#27272a", // zinc-800
        },
        style,
      ]}
    >
      <SafeAreaView edges={["bottom", "top"]} className="flex-1">
        <ScrollView
          className="flex-1"
          contentContainerStyle={{ padding: 16 }}
          showsVerticalScrollIndicator={false}
        >
          <View className="flex-row items-center justify-between mb-6 mt-12">
            <Text className="text-xl font-bold text-white">情報</Text>
          </View>

          <View className="gap-6">
            <View>
              <View className="flex-row items-center gap-2 mb-2">
                <Ionicons name="calendar-outline" size={16} color="#a1a1aa" />
                <Text className="text-zinc-400 font-medium">日時</Text>
              </View>
              <Text className="text-zinc-200 text-base">
                {new Date(currentAsset.creationTime).toLocaleString()}
              </Text>
            </View>

            <View>
              <View className="flex-row items-center gap-2 mb-2">
                <Ionicons name="image-outline" size={16} color="#a1a1aa" />
                <Text className="text-zinc-400 font-medium">詳細</Text>
              </View>
              <Text className="text-zinc-200 text-base">
                {currentAsset.width} x {currentAsset.height}
              </Text>
              <Text className="text-zinc-400 text-sm mt-1">
                {currentAssetInfo
                  ? formatSize(currentAssetInfo.localUri ? undefined : 0)
                  : "Loading..."}{" "}
                • {currentAsset.filename}
              </Text>
            </View>

            {/* Window Info */}
            {(dbHostInfo?.windowTitle || dbHostInfo?.processName) && (
              <View>
                <View className="flex-row items-center gap-2 mb-2">
                  <Ionicons name="desktop-outline" size={16} color="#a1a1aa" />
                  <Text className="text-zinc-400 font-medium">アプリケーション</Text>
                </View>
                {dbHostInfo.windowTitle && (
                  <Text className="text-zinc-200 text-base">{dbHostInfo.windowTitle}</Text>
                )}
                {dbHostInfo.processName && (
                  <Text className="text-zinc-400 text-sm mt-1">{dbHostInfo.processName}</Text>
                )}
                {dbHostInfo.processPath && (
                  <Text
                    className="text-zinc-500 text-xs mt-1"
                    numberOfLines={1}
                    ellipsizeMode="middle"
                  >
                    {dbHostInfo.processPath}
                  </Text>
                )}
              </View>
            )}
          </View>

          {/* AI Analysis */}
          <View className="mt-8 mb-8">
            <View className="flex-row items-center justify-between mb-4">
              <Text className="text-white text-base font-bold flex flex-row items-center gap-2">
                <Ionicons name="sparkles" size={16} color="#a855f7" /> AI Analysis
              </Text>
            </View>

            {analysis ? (
              <View className="gap-4">
                <AnalysisViewer analysis={analysis} />
                {isAnalyzing && (
                  <View className="flex-row items-center justify-center p-2 opacity-70">
                    <ActivityIndicator size="small" color="#a855f7" />
                    <Text className="text-zinc-400 text-xs ml-2">Updating...</Text>
                  </View>
                )}
              </View>
            ) : isAnalyzing ? (
              <View className="items-center justify-center py-8 gap-3">
                <ActivityIndicator size="large" color="#a855f7" />
                <Text className="text-zinc-500 text-sm animate-pulse">
                  Analyzing image context...
                </Text>
              </View>
            ) : (
              <View className="bg-zinc-900 border border-zinc-800 rounded-lg p-4 items-center gap-3">
                <Text className="text-zinc-400 text-sm text-center">
                  Get insights about the scene, characters, and dialogue using AI.
                </Text>
                <Button
                  variant="outline"
                  className="w-full border-zinc-700 bg-zinc-800"
                  onPress={() => hostId && onRequestAnalyze?.(hostId, 512)}
                >
                  <View className="flex-row items-center gap-2">
                    <Ionicons name="sparkles" size={16} color="#c084fc" />
                    <Text className="text-zinc-200">Analyze Screenshot</Text>
                  </View>
                </Button>
              </View>
            )}
          </View>
        </ScrollView>
      </SafeAreaView>
    </Animated.View>
  )
}
