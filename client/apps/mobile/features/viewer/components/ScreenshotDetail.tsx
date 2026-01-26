import { Ionicons } from "@expo/vector-icons"
import * as MediaLibrary from "expo-media-library"
import { forwardRef, useImperativeHandle, useEffect, useState } from "react"
import { View, Image, Text, Dimensions } from "react-native"
import { SafeAreaView } from "react-native-safe-area-context"
import Animated, {
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  interpolate,
  runOnJS,
  Easing,
  FadeIn,
  FadeOut
} from "react-native-reanimated"

import { Button } from "@/components/ui/button"

export interface AssetOrigin {
  x: number
  y: number
  width: number
  height: number
}

interface ScreenshotDetailProps {
  asset: MediaLibrary.Asset
  origin?: AssetOrigin | null
  onClose: () => void
}

export interface ScreenshotDetailRef {
  close: () => void
}

export const ScreenshotDetail = forwardRef<ScreenshotDetailRef, ScreenshotDetailProps>(
  ({ asset, origin, onClose }, ref) => {
  const [assetInfo, setAssetInfo] = useState<MediaLibrary.AssetInfo | null>(null)
  
  // Animation state: 0 = closed (at thumbnail), 1 = open (fullscreen)
  const anim = useSharedValue(0)
  const windowDimensions = Dimensions.get("window")

  useEffect(() => {
    // Start animation on mount
    anim.value = withTiming(1, {
      duration: 350,
      easing: Easing.out(Easing.cubic),
    })

    const fetchInfo = async () => {
      const info = await MediaLibrary.getAssetInfoAsync(asset)
      setAssetInfo(info)
    }
    fetchInfo()
  }, [asset])

  const handleClose = () => {
    // Reverse animation on close
    anim.value = withTiming(0, {
      duration: 300,
      easing: Easing.in(Easing.cubic),
    }, (finished) => {
      if (finished) {
        runOnJS(onClose)()
      }
    })
  }

  useImperativeHandle(ref, () => ({
    close: handleClose
  }))

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

  // Image geometry animation
  const imageContainerStyle = useAnimatedStyle(() => {
    if (!origin) {
      // Fallback if no origin (just simple scale/fade handled by entering prop if we used it, 
      // but here we manually interpolate for consistency)
      return {
        opacity: anim.value,
        transform: [{ scale: interpolate(anim.value, [0, 1], [0.8, 1]) }],
        top: 0,
        left: 0,
        width: "100%",
        height: "100%",
      }
    }

    // Interpolate from origin rect to full screen rect
    // Note: We animate the container to cover the screen, but we want the image to appear to expand.
    // Simplifying assumption: The destination is the safe area center, but for now let's just use full screen as target.
    
    // Target: Full Screen
    const targetX = 0
    const targetY = 0
    const targetW = windowDimensions.width
    const targetH = windowDimensions.height

    return {
      position: "absolute",
      left: interpolate(anim.value, [0, 1], [origin.x, targetX]),
      top: interpolate(anim.value, [0, 1], [origin.y, targetY]),
      width: interpolate(anim.value, [0, 1], [origin.width, targetW]),
      height: interpolate(anim.value, [0, 1], [origin.height, targetH]),
      // Make sure it stays separate from backdrop
      zIndex: 10,
    }
  })

  // Content (Header/Info) opacity - delay it slightly so it doesn't clutter the expansion
  const contentStyle = useAnimatedStyle(() => ({
    opacity: interpolate(anim.value, [0.5, 1], [0, 1]),
    transform: [{ translateY: interpolate(anim.value, [0, 1], [20, 0]) }],
  }))

  return (
    <View className="flex-1 bg-transparent absolute inset-0 z-50">
      {/* Dark Backdrop */}
      <Animated.View 
        style={[{ flex: 1, backgroundColor: "#09090b" }, backdropStyle]} 
        className="absolute inset-0"
      />

      {/* Calculating Safe Area manually for the Image Container to ensure full bleed if needed, 
          but actually we want the image to end up "contained" in the view.
          For the "expand" effect, it's best if the image container expands to the final viewer area.
      */}
      
      {/* Helper to clip during animation if needed, though overflow visible usually looks smoother for expansion */}
      <Animated.View style={[imageContainerStyle, { overflow: 'hidden' }]}> 
         <Image
            source={{ uri: asset.uri }}
            style={{ width: "100%", height: "100%" }}
            resizeMode="contain"
          />
      </Animated.View>


      {/* Overlay UI (Header + Info) - Rendered over the image */}
      <SafeAreaView 
        className="flex-1" 
        edges={["top", "left", "right"]}
        pointerEvents="box-none"
        style={{ zIndex: 20 }}
      >
        <Animated.View style={[{ flex: 1 }, contentStyle]}>
          <View className="flex-1">
            {/* Header */}
            <View className="flex-row items-center gap-2 px-4 py-2 bg-zinc-950/50 absolute top-0 left-0 right-0 z-20">
              <Button
                variant="ghost"
                size="icon"
                className="rounded-full"
                onPress={handleClose}
              >
                <Ionicons name="arrow-back" size={24} color="#a1a1aa" />
              </Button>
              <View className="flex-1">
                 <Text className="text-white text-lg font-medium ml-2">
                   Screenshot Details
                 </Text>
              </View>
            </View>

            {/* Main Content Area (Transparent to let image show through) */}
            <View className="flex-1" />

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
                    {new Date(asset.creationTime).toLocaleString()}
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
                    {asset.width} x {asset.height}
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
                    {assetInfo
                      ? formatSize(assetInfo.localUri ? undefined : 0)
                      : "Loading..."}
                  </Text>
                </View>
              </View>
            </View>
          </View>
        </Animated.View>
      </SafeAreaView>
    </View>
  )
})
