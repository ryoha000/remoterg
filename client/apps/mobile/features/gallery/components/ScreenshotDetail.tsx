import { Ionicons } from "@expo/vector-icons"
import { BlurView } from "expo-blur"
import * as MediaLibrary from "expo-media-library"
import { StatusBar } from "expo-status-bar"
import { forwardRef, useImperativeHandle, useEffect, useState, useRef, useMemo } from "react"
import {
  View,
  Text,
  Dimensions,
  TouchableWithoutFeedback,
  ActivityIndicator,
  ScrollView,
} from "react-native"
import { Image } from "expo-image"
import { Gesture, GestureDetector, FlatList } from "react-native-gesture-handler"
import Animated, {
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  interpolate,
  runOnJS,
  Easing,
  withSpring,
  Extrapolation,
} from "react-native-reanimated"
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { useAnalysis } from "@/db/queries/use-analysis"
import { useHostId } from "@/db/queries/use-screenshots"

import { AnalysisResult } from "../types"
import { AnalysisViewer } from "./AnalysisViewer"

export type { AnalysisResult }

export interface AssetOrigin {
  x: number
  y: number
  width: number
  height: number
}

interface ScreenshotDetailProps {
  assets: MediaLibrary.Asset[]
  initialIndex: number
  origin?: AssetOrigin | null
  onClose: () => void
  getAssetOrigin: (id: string) => Promise<AssetOrigin | null>
  onCurrentIndexChange: (index: number) => void
  onRequestAnalyze?: (id: string, max_edge?: number) => void
  analysisResults?: Record<string, AnalysisResult>
  isAnalyzingMap?: Record<string, boolean>
  assetInfoMap: Record<string, MediaLibrary.AssetInfo>
  onAssetInfoLoaded: (id: string, info: MediaLibrary.AssetInfo) => void
}

export interface ScreenshotDetailRef {
  close: () => void
}

const AnimatedImage = Animated.createAnimatedComponent(Image)

const INFO_PANEL_WIDTH_PCT = 0.35 // 35% width for info panel
// Adjust for portrait mode - in portrait 35% might be too narrow for text, but 60% image is also small.
// Let's assume the user knows what they want (Google Photos style).
// Google Photos actually pushes the image UP in portrait, and Left in Landscape?
// Or maybe just overlays? The user specifically said "Image small left, Info right".
// We will follow that instruction.

// Item Component for FlatList
const ScreenshotPage = ({
  asset,
  isActive,
  onTap,
  onDismiss,
  shouldAnimateEntry,
  origin,
  closingOrigin,
  isClosing,
  windowDimensions,
  isInfoOpen,
  uri,
}: {
  asset: MediaLibrary.Asset
  isActive: boolean
  onTap: () => void
  onDismiss: () => void
  shouldAnimateEntry: boolean
  origin?: AssetOrigin | null
  closingOrigin?: AssetOrigin | null
  isClosing: boolean
  uri?: string
  windowDimensions: { width: number; height: number }
  isInfoOpen: boolean
}) => {
  // Animation for entry (only if this is the initial active item)
  const entryAnim = useSharedValue(shouldAnimateEntry ? 0 : 1)
  const closingAnim = useSharedValue(0)
  const infoOpenAnim = useSharedValue(isInfoOpen ? 1 : 0)

  // Gesture Values
  const translationY = useSharedValue(0)
  const isDragging = useSharedValue(false)

  useEffect(() => {
    if (shouldAnimateEntry) {
      entryAnim.value = withTiming(1, {
        duration: 350,
        easing: Easing.out(Easing.cubic),
      })
    }
  }, [])

  useEffect(() => {
    if (isClosing && isActive) {
      closingAnim.value = withTiming(1, {
        duration: 350,
        easing: Easing.out(Easing.cubic),
      })
    }
  }, [isClosing, isActive])

  useEffect(() => {
    infoOpenAnim.value = withTiming(isInfoOpen ? 1 : 0, {
      duration: 300,
      easing: Easing.inOut(Easing.cubic),
    })
  }, [isInfoOpen])

  const panGesture = Gesture.Pan()
    .enabled(!isClosing)
    .activeOffsetY([-10, 10])
    .onUpdate((e) => {
      translationY.value = e.translationY
      isDragging.value = true
    })
    .onEnd((e) => {
      isDragging.value = false
      if (Math.abs(e.translationY) > 100 || Math.abs(e.velocityY) > 500) {
        runOnJS(onDismiss)()
      } else {
        translationY.value = withSpring(0)
      }
    })

  const tapGesture = Gesture.Tap()
    .enabled(!isClosing)
    .onEnd(() => {
      runOnJS(onTap)()
    })

  const composedGesture = Gesture.Exclusive(panGesture, tapGesture)

  const containerStyle = useAnimatedStyle(() => {
    let translateX = 0
    let translateY = translationY.value
    let scale = 1
    let width = windowDimensions.width
    let height = windowDimensions.height

    // -- Layout Shift Logic (Info Open) --
    const infoPanelWidth = windowDimensions.width * INFO_PANEL_WIDTH_PCT
    const imageAreaWidth = windowDimensions.width - infoPanelWidth

    // -- Layout Shift Logic (Info Open) --
    // Animate width to shrink
    const currentWidth = interpolate(
      infoOpenAnim.value,
      [0, 1],
      [windowDimensions.width, imageAreaWidth],
    )

    width = currentWidth

    // -- Drag Scale --
    const dragScale = interpolate(
      Math.abs(translationY.value),
      [0, windowDimensions.height],
      [1, 0.8],
      Extrapolation.CLAMP,
    )

    if (isClosing && isActive && closingOrigin) {
      // Closing Animation Logic
      const targetX = closingOrigin.x
      const targetY = closingOrigin.y
      const targetWidth = closingOrigin.width
      const targetHeight = closingOrigin.height

      // Interpolate to target
      // We start from 0 translation (relative to our current container) to targetX which is absolute.
      // But our container is at 0,0.
      translateX = interpolate(closingAnim.value, [0, 1], [translateX, targetX])
      translateY = interpolate(closingAnim.value, [0, 1], [translateY, targetY])
      width = interpolate(closingAnim.value, [0, 1], [width, targetWidth])
      height = interpolate(closingAnim.value, [0, 1], [height, targetHeight])
    } else if (shouldAnimateEntry && entryAnim.value < 1) {
      if (origin) {
        const targetX = 0
        const targetY = 0

        translateX = interpolate(entryAnim.value, [0, 1], [origin.x, targetX])
        translateY = interpolate(entryAnim.value, [0, 1], [origin.y, targetY]) + translationY.value
        width = interpolate(entryAnim.value, [0, 1], [origin.width, windowDimensions.width])
        height = interpolate(entryAnim.value, [0, 1], [origin.height, windowDimensions.height])
      } else {
        scale = interpolate(entryAnim.value, [0, 1], [0.8, 1]) * dragScale
      }
    } else {
      scale = dragScale
    }

    return {
      position: "absolute",
      left: 0,
      top: 0,
      width,
      height,
      transform: [{ translateX }, { translateY }, { scale }],
      zIndex: 10,
      overflow: "hidden",
    }
  })

  // Fade out only if not the active closing item
  const opacityStyle = useAnimatedStyle(() => {
    if (isClosing && !isActive) {
      return { opacity: withTiming(0, { duration: 200 }) }
    }
    return { opacity: 1 }
  })

  return (
    <View
      style={{ width: windowDimensions.width, height: windowDimensions.height, overflow: "hidden" }}
      pointerEvents={isClosing ? "none" : "auto"}
    >
      <GestureDetector gesture={composedGesture}>
        <Animated.View style={[containerStyle, opacityStyle]}>
          <AnimatedImage
            source={{ 
              uri: uri || asset.uri,
              width: asset.width,
              height: asset.height
            }}
            style={{ width: "100%", height: "100%" }}
            contentFit="contain"
            cachePolicy="memory-disk"
          />
        </Animated.View>
      </GestureDetector>
    </View>
  )
}

export const ScreenshotDetail = forwardRef<ScreenshotDetailRef, ScreenshotDetailProps>(
  (
    {
      assets,
      initialIndex,
      origin,
      onClose,
      getAssetOrigin,
      onCurrentIndexChange,
      onRequestAnalyze,
      analysisResults,
      isAnalyzingMap,
      assetInfoMap,
      onAssetInfoLoaded,
    },
    ref,
  ) => {
    const [currentIndex, setCurrentIndex] = useState(initialIndex)

    // Closing state
    const [isClosing, setIsClosing] = useState(false)
    const [closingOrigin, setClosingOrigin] = useState<AssetOrigin | null>(null)

    // Animation state: 0 = closed (at thumbnail), 1 = open (fullscreen)
    // We use this primarily for the backdrop opacity now
    const anim = useSharedValue(0)

    // Info/Overlay state: true = Info Visible, false = Fullscreen
    const [showInfo, setShowInfo] = useState(false)
    // Default to false (Fullscreen) as per "Initially object-fit: contain" request

    const currentAsset = assets[currentIndex]
    const currentAssetInfo = assetInfoMap[currentAsset.id]

    // Load persisted analysis & Resolve Host ID
    const { data: dbHostInfo } = useHostId(currentAsset.id)
    const hostId = useMemo(() => {
      if (dbHostInfo) return dbHostInfo.hostId
      return currentAsset.filename?.replace(/\.[^/.]+$/, "") || currentAsset.id
    }, [dbHostInfo, currentAsset])

    // Load analysis using Local ID
    const { data: savedAnalysis } = useAnalysis(currentAsset.id)

    const sessionAnalysis = hostId ? analysisResults?.[hostId] : null
    const analysis = sessionAnalysis || savedAnalysis

    const isAnalyzing = hostId ? isAnalyzingMap?.[hostId] : false

    // Animated value for toggling UI
    const infoAnim = useSharedValue(0)

    const windowDimensions = Dimensions.get("window")
    const insets = useSafeAreaInsets()

    useEffect(() => {
      // Start global animation (backdrop)
      anim.value = withTiming(1, {
        duration: 350,
        easing: Easing.out(Easing.cubic),
      })
    }, [])

    useEffect(() => {
      const fetchInfo = async () => {
        // Fetch current and neighbors
        const indicesToFetch = [currentIndex, currentIndex - 1, currentIndex + 1].filter(
          (i) => i >= 0 && i < assets.length,
        )

        for (const i of indicesToFetch) {
          const asset = assets[i]
          if (!asset || assetInfoMap[asset.id]) continue

          try {
            const info = await MediaLibrary.getAssetInfoAsync(asset)
            if (info) {
              onAssetInfoLoaded(asset.id, info)
            }
          } catch (e) {
            console.error("Failed to load asset info for", asset.id, e)
          }
        }
      }
      fetchInfo()
    }, [currentIndex, assets, assetInfoMap, onAssetInfoLoaded])

    // Sync overlay animation with state
    useEffect(() => {
      infoAnim.value = withTiming(showInfo && !isClosing ? 1 : 0, {
        duration: 300,
        easing: Easing.inOut(Easing.cubic),
      })
    }, [showInfo, isClosing])

    const handleClose = async () => {
      // 1. Prepare closing
      setIsClosing(true)

      // 2. Hide overlay immediately (via effect)
      setShowInfo(false)

      // 3. Get target origin
      const currentAsset = assets[currentIndex]
      // Ensure we scroll to it first (though onCurrentIndexChange should have handled it mostly)
      onCurrentIndexChange(currentIndex)

      const origin = await getAssetOrigin(currentAsset.id)
      setClosingOrigin(origin)

      // 4. Fade out backdrop
      anim.value = withTiming(
        0,
        {
          duration: 350,
          easing: Easing.out(Easing.cubic),
        },
        (finished) => {
          if (finished) {
            runOnJS(onClose)()
          }
        },
      )
    }

    useImperativeHandle(ref, () => ({
      close: handleClose,
    }))

    const toggleInfo = () => {
      setShowInfo((prev) => !prev)
    }

    const formatSize = (bytes?: number) => {
      if (!bytes) return "Unknown"
      const k = 1024
      const sizes = ["B", "KB", "MB", "GB"]
      const i = Math.floor(Math.log(bytes) / Math.log(k))
      return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i]
    }

    // Backdrop opacity
    const backdropStyle = useAnimatedStyle(() => ({
      opacity: anim.value,
    }))

    // Info Panel Animation
    // Slides in from Right
    const infoPanelStyle = useAnimatedStyle(() => {
      const width = windowDimensions.width * INFO_PANEL_WIDTH_PCT
      const translateX = interpolate(infoAnim.value, [0, 1], [width, 0])
      // Also fade
      const opacity = interpolate(infoAnim.value, [0, 0.5, 1], [0, 0, 1])

      return {
        transform: [{ translateX }],
        opacity,
        width,
      }
    })

    // Header/Bottom Gradient Animation
    // They just fade in/out, and also shift width for Header
    const overlayControlsStyle = useAnimatedStyle(() => ({
      opacity: infoAnim.value,
      pointerEvents: infoAnim.value > 0.5 ? "auto" : "none",
      right: interpolate(
        infoAnim.value,
        [0, 1],
        [0, windowDimensions.width * INFO_PANEL_WIDTH_PCT],
      ),
    }))

    const onViewableItemsChanged = useRef(({ viewableItems }: any) => {
      if (viewableItems.length > 0) {
        const newIndex = viewableItems[0].index
        setCurrentIndex(newIndex)
        if (newIndex !== null && newIndex !== undefined) {
          onCurrentIndexChange(newIndex)
        }
      }
    }).current

    return (
      <View className="flex-1 bg-transparent absolute inset-0 z-50">
        <StatusBar hidden={!showInfo} style="light" />
        {/* Dark Backdrop */}
        <Animated.View
          style={[{ flex: 1, backgroundColor: "#000000" }, backdropStyle]} // Changed to pure black for better photo viewer feel
          className="absolute inset-0"
        />

        {/* Carousel */}
        <FlatList
          data={assets}
          horizontal
          pagingEnabled
          initialScrollIndex={initialIndex}
          getItemLayout={(data, index) => ({
            length: windowDimensions.width,
            offset: windowDimensions.width * index,
            index,
          })}
          showsHorizontalScrollIndicator={false}
          onViewableItemsChanged={onViewableItemsChanged}
          viewabilityConfig={{ itemVisiblePercentThreshold: 50 }}
          scrollEnabled={!isClosing} // Maybe disable scroll when info is open? Google Photos allows it.
          extraData={assetInfoMap}
          renderItem={({ item, index }) => {
            const isCurrent = index === currentIndex
            const info = assetInfoMap[item.id]
            // Use cached info localUri if available, otherwise asset.uri
            // Only prioritize localUri if we have it.
            // When isCurrent is true, we try to use the high res one.
            // Actually, we can use high res for ANY item if we have it loaded.
            const uri = info?.localUri || info?.uri

             return (
              <ScreenshotPage
                asset={item}
                isActive={isCurrent}
                windowDimensions={windowDimensions}
                onTap={toggleInfo}
                onDismiss={handleClose}
                shouldAnimateEntry={index === initialIndex}
                origin={index === initialIndex ? origin : null}
                isClosing={isClosing}
                closingOrigin={closingOrigin}
                isInfoOpen={showInfo} // Pass state
                uri={uri}
              />
            )
          }}
          keyExtractor={(item) => item.id}
        />

        {/* Header Overlay */}
        <SafeAreaView
          className="flex-1 absolute inset-0 pointer-events-box-none"
          edges={["top", "bottom", "left", "right"]}
          pointerEvents="box-none"
          style={{ zIndex: 20 }}
        >
          {/* Header Controls */}
          <Animated.View
            style={[
              {
                position: "absolute",
                top: 0,
                left: 0,
                zIndex: 30,
                paddingTop: insets.top,
                backgroundColor: "rgba(0,0,0,0.4)",
              },
              overlayControlsStyle,
            ]}
          >
            <View className="flex-row items-center gap-2 px-4 py-2">
              <Button
                variant="ghost"
                size="icon"
                className="rounded-full bg-black/20"
                onPress={handleClose}
              >
                <Ionicons name="arrow-back" size={24} color="#ffffff" />
              </Button>
              <View className="flex-1" />
              <Button variant="ghost" size="icon" className="rounded-full bg-black/20">
                <Ionicons name="star-outline" size={24} color="#ffffff" />
              </Button>
              <Button variant="ghost" size="icon" className="rounded-full bg-black/20">
                <Ionicons name="ellipsis-vertical" size={24} color="#ffffff" />
              </Button>
            </View>
          </Animated.View>

          {/* Info Panel (Right Side) */}
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
              infoPanelStyle,
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
                      {currentAssetInfo ? formatSize(currentAssetInfo.localUri ? undefined : 0) : "Loading..."} •{" "}
                      {currentAsset.filename}
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

              {/* Bottom Actions inside Info Panel? Or separate?
                      Google Photos has bottom bar actions (Share, Edit, Lens, Delete) separate from Info Panel.
                      But in our requested UI, let's keep it simple.
                  */}
            </SafeAreaView>
          </Animated.View>

          {/* Bottom Actions Overlay (Separate from Info Panel, shows when Info/Overlay is active) */}
          <Animated.View
            style={[
              {
                position: "absolute",
                bottom: 0,
                left: 0,
                // right handled by overlayControlsStyle
                // If Layout Shift moves image to left, these buttons should probably be under the image or centered in the image area?
                // If InfoPanel is opaque, we should stop at its edge.
              },
              overlayControlsStyle,
            ]}
            pointerEvents="box-none"
          >
            <View className="bg-black/60 backdrop-blur-md pb-8 pt-4 flex-row justify-around items-center border-t border-white/10">
              <View className="items-center gap-1">
                <Ionicons name="share-outline" size={20} color="white" />
                <Text className="text-white text-xs">共有</Text>
              </View>
              <View className="items-center gap-1">
                <Ionicons name="options-outline" size={20} color="white" />
                <Text className="text-white text-xs">編集</Text>
              </View>
              <View className="items-center gap-1">
                <Ionicons name="trash-outline" size={20} color="white" />
                <Text className="text-white text-xs">ゴミ箱</Text>
              </View>
            </View>
          </Animated.View>
        </SafeAreaView>
      </View>
    )
  },
)
