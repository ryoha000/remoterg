import { Image } from "expo-image"
import * as MediaLibrary from "expo-media-library"
import { useEffect } from "react"
import { View } from "react-native"
import { Gesture, GestureDetector } from "react-native-gesture-handler"
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

import { AssetOrigin } from "@/features/gallery/types"

const AnimatedImage = Animated.createAnimatedComponent(Image)
const INFO_PANEL_WIDTH_PCT = 0.35

interface ScreenshotPageProps {
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
}

export const ScreenshotPage = ({
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
}: ScreenshotPageProps) => {
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
              height: asset.height,
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
