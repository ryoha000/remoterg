import { Ionicons } from "@expo/vector-icons"
import * as MediaLibrary from "expo-media-library"
import { StatusBar } from "expo-status-bar"
import { forwardRef, useImperativeHandle, useEffect, useState, useRef, useMemo } from "react"
import { View, Image, Text, Dimensions, TouchableWithoutFeedback } from "react-native"
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
import { SafeAreaView } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"

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
}

export interface ScreenshotDetailRef {
  close: () => void
}

const AnimatedImage = Animated.createAnimatedComponent(Image)

// Item Component for FlatList
const ScreenshotPage = ({
  asset,
  isActive,
  onTap,
  onDismiss,
  shouldAnimateEntry,
  origin,
  windowDimensions,
}: {
  asset: MediaLibrary.Asset
  isActive: boolean
  onTap: () => void
  onDismiss: () => void
  shouldAnimateEntry: boolean
  origin?: AssetOrigin | null
  windowDimensions: { width: number; height: number }
}) => {
  // Animation for entry (only if this is the initial active item)
  const entryAnim = useSharedValue(shouldAnimateEntry ? 0 : 1)

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

  const panGesture = Gesture.Pan()
    .activeOffsetY([-10, 10]) // Don't activate immediately, wait for vertical movement
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
    .onEnd(() => {
        runOnJS(onTap)()
    })

  // Pan takes precedence over Tap. If Pan activates, Tap fails.
  const composedGesture = Gesture.Exclusive(panGesture, tapGesture)

  const containerStyle = useAnimatedStyle(() => {
    // If dragging, follow gesture.
    // If entering, interpolate from origin.

    let translateX = 0
    let translateY = translationY.value
    let scale = 1
    let width = windowDimensions.width
    let height = windowDimensions.height

    // Scale down slightly when dragging logic can be added here
    const dragScale = interpolate(
      Math.abs(translationY.value),
      [0, windowDimensions.height],
      [1, 0.8],
      Extrapolation.CLAMP,
    )

    if (shouldAnimateEntry && entryAnim.value < 1) {
      // Entry Animation Logic
      if (origin) {
        const targetX = 0
        const targetY = 0

        translateX = interpolate(entryAnim.value, [0, 1], [origin.x, targetX])
        translateY = interpolate(entryAnim.value, [0, 1], [origin.y, targetY]) + translationY.value
        width = interpolate(entryAnim.value, [0, 1], [origin.width, windowDimensions.width])
        height = interpolate(entryAnim.value, [0, 1], [origin.height, windowDimensions.height])
        // We handle scale differently here, mostly relies on width/height change
      } else {
        // Fallback zoom
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
    }
  })

  return (
    <View
      style={{ width: windowDimensions.width, height: windowDimensions.height, overflow: "hidden" }}
    >
      <GestureDetector gesture={composedGesture}>
        <Animated.View style={containerStyle}>
          <AnimatedImage
            source={{ uri: asset.uri }}
            style={{ width: "100%", height: "100%" }}
            resizeMode="contain"
          />
        </Animated.View>
      </GestureDetector>
    </View>
  )
}

export const ScreenshotDetail = forwardRef<ScreenshotDetailRef, ScreenshotDetailProps>(
  ({ assets, initialIndex, origin, onClose }, ref) => {
    const [currentIndex, setCurrentIndex] = useState(initialIndex)
    const [assetInfo, setAssetInfo] = useState<MediaLibrary.AssetInfo | null>(null)

    // Animation state: 0 = closed (at thumbnail), 1 = open (fullscreen)
    // We use this primarily for the backdrop opacity now
    const anim = useSharedValue(0)

    // Overlay state: 0 = hidden, 1 = visible
    const overlayAnim = useSharedValue(1)
    const [showOverlay, setShowOverlay] = useState(true)

    const windowDimensions = Dimensions.get("window")

    useEffect(() => {
      // Start global animation (backdrop)
      anim.value = withTiming(1, {
        duration: 350,
        easing: Easing.out(Easing.cubic),
      })
    }, [])

    useEffect(() => {
      const fetchInfo = async () => {
        const info = await MediaLibrary.getAssetInfoAsync(assets[currentIndex])
        setAssetInfo(info)
      }
      fetchInfo()
    }, [currentIndex, assets])

    // Sync overlay animation with state
    useEffect(() => {
      overlayAnim.value = withTiming(showOverlay ? 1 : 0, {
        duration: 200,
      })
    }, [showOverlay])

    const handleClose = () => {
      // Fade out backdrop
      anim.value = withTiming(
        0,
        {
          duration: 250,
          easing: Easing.in(Easing.cubic),
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

    const toggleOverlay = () => {
      setShowOverlay((prev) => !prev)
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

    // Content (Header/Info) opacity
    const contentStyle = useAnimatedStyle(() => {
      // Only show content when fully open (approx)
      const expandOpacity = interpolate(anim.value, [0.5, 1], [0, 1])
      return {
        opacity: expandOpacity * overlayAnim.value,
        transform: [{ translateY: interpolate(overlayAnim.value, [0, 1], [-20, 0]) }],
        // changed translate to be controlled by overlayAnim for cleaner toggle
      }
    })

    const onViewableItemsChanged = useRef(({ viewableItems }: any) => {
      if (viewableItems.length > 0) {
        setCurrentIndex(viewableItems[0].index)
      }
    }).current

    const currentAsset = assets[currentIndex]

    return (
      <View className="flex-1 bg-transparent absolute inset-0 z-50">
        <StatusBar hidden={!showOverlay} />
        {/* Dark Backdrop */}
        <Animated.View
          style={[{ flex: 1, backgroundColor: "#09090b" }, backdropStyle]}
          className="absolute inset-0"
        />

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
          renderItem={({ item, index }) => (
            <ScreenshotPage
              asset={item}
              isActive={index === currentIndex}
              windowDimensions={windowDimensions}
              onTap={toggleOverlay}
              onDismiss={handleClose}
              // Only animate entry for the initially selected item
              shouldAnimateEntry={index === initialIndex}
              origin={index === initialIndex ? origin : null}
            />
          )}
          keyExtractor={(item) => item.id}
        />

        {/* Overlay UI (Header + Info) - Rendered over the image */}
        <SafeAreaView
          className="flex-1 absolute inset-0 pointer-events-box-none"
          edges={["top", "left", "right"]}
          pointerEvents="box-none"
          style={{ zIndex: 20 }}
        >
          <Animated.View
            style={[{ flex: 1 }, contentStyle]}
            pointerEvents={showOverlay ? "box-none" : "none"}
          >
            <View className="flex-1 justify-between" pointerEvents="box-none">
              {/* Header */}
              <View className="flex-row items-center gap-2 px-4 py-2 bg-zinc-950/50">
                <Button variant="ghost" size="icon" className="rounded-full" onPress={handleClose}>
                  <Ionicons name="arrow-back" size={24} color="#a1a1aa" />
                </Button>
                <View className="flex-1">
                  <Text className="text-white text-lg font-medium ml-2">
                    {currentIndex + 1} / {assets.length}
                  </Text>
                </View>
              </View>

              {/* Info Panel */}
              <View className="bg-zinc-900/90 border-t border-zinc-800 p-6 pb-8 backdrop-blur-md">
                <Text className="text-lg font-semibold text-white mb-4">Metadata</Text>
                <View className="gap-4">
                  <View>
                    <View className="flex-row items-center gap-2 mb-1">
                      <Ionicons name="calendar-outline" size={14} color="#71717a" />
                      <Text className="text-zinc-500 text-xs uppercase tracking-wider font-medium">
                        Timestamp
                      </Text>
                    </View>
                    <Text className="text-zinc-300 font-mono text-sm">
                      {new Date(currentAsset.creationTime).toLocaleString()}
                    </Text>
                  </View>

                  <View>
                    <View className="flex-row items-center gap-2 mb-1">
                      <Ionicons name="resize-outline" size={14} color="#71717a" />
                      <Text className="text-zinc-500 text-xs uppercase tracking-wider font-medium">
                        Dimensions
                      </Text>
                    </View>
                    <Text className="text-zinc-300 font-mono text-sm">
                      {currentAsset.width} x {currentAsset.height}
                    </Text>
                  </View>

                  <View>
                    <View className="flex-row items-center gap-2 mb-1">
                      <Ionicons name="hardware-chip-outline" size={14} color="#71717a" />
                      <Text className="text-zinc-500 text-xs uppercase tracking-wider font-medium">
                        Size
                      </Text>
                    </View>
                    <Text className="text-zinc-300 font-mono text-sm">
                      {assetInfo ? formatSize(assetInfo.localUri ? undefined : 0) : "Loading..."}
                    </Text>
                  </View>
                </View>
              </View>
            </View>
          </Animated.View>
        </SafeAreaView>
      </View>
    )
  },
)
