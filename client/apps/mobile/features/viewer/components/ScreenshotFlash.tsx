import { Image } from "expo-image"
import { forwardRef, useImperativeHandle, useState, useEffect } from "react"
import { Image as RNImage } from "react-native"
import { StyleSheet, View, useWindowDimensions } from "react-native"
import Animated, {
  useAnimatedStyle,
  useSharedValue,
  withSequence,
  withTiming,
  withDelay,
  runOnJS,
  Easing,
  withSpring,
} from "react-native-reanimated"

export interface ScreenshotFlashHandle {
  setContentSize: (width: number, height: number) => void
  triggerFlash: () => void
  showResult: (uri: string) => void
}

export const ScreenshotFlash = forwardRef<ScreenshotFlashHandle, {}>((_, ref) => {
  const [screenshotUri, setScreenshotUri] = useState<string | null>(null)

  // Store the source content dimensions (Video track size)
  const [contentSize, setContentSize] = useState<{ width: number; height: number } | null>(null)

  // Derived display layout
  const [layout, setLayout] = useState<{ width: number; height: number } | null>(null)

  // Flash opacity
  const flashOpacity = useSharedValue(0)

  // Screenshot image animation values
  const imageOpacity = useSharedValue(0)
  const imageScale = useSharedValue(1)
  const imageTranslateX = useSharedValue(0)
  const imageTranslateY = useSharedValue(0)

  const { width: screenW, height: screenH } = useWindowDimensions()

  const calculateLayout = (cW: number, cH: number, sW: number, sH: number) => {
    const screenRatio = sW / sH
    const contentRatio = cW / cH

    let displayW, displayH

    if (contentRatio > screenRatio) {
      // Wider than screen -> fit width
      displayW = sW
      displayH = sW / contentRatio
    } else {
      // Taller -> fit height
      displayH = sH
      displayW = sH * contentRatio
    }
    return { width: displayW, height: displayH }
  }

  // Update layout whenever contentSize or Screen dimensions change
  useEffect(() => {
    if (contentSize) {
      setLayout(calculateLayout(contentSize.width, contentSize.height, screenW, screenH))
    }
  }, [contentSize, screenW, screenH])

  useImperativeHandle(ref, () => ({
    setContentSize: (width: number, height: number) => {
      setContentSize({ width, height })
    },

    triggerFlash: () => {
      // Flash effect immediately
      flashOpacity.value = withSequence(
        withTiming(0.8, { duration: 50 }),
        withTiming(0, { duration: 100 }),
      )
    },

    showResult: (uri: string) => {
      setScreenshotUri(uri)

      // Reset values
      imageOpacity.value = 1
      imageScale.value = 1
      imageTranslateX.value = 0
      imageTranslateY.value = 0

      // We use the current layout derived from contentSize.
      // If contentSize was never set, we might need to fallback to measuring the image?
      // But per request "set at connection time", we should rely on contentSize.
      // If layout is missing (no track info), we fallback to screen or measure.

      const prepareAnimation = (displayLayout: { width: number; height: number }) => {
        // 3. Shrink animation
        const targetScale = 0.2
        const currentDisplayW = displayLayout.width
        const currentDisplayH = displayLayout.height

        const margin = 20
        const targetX = -screenW / 2 + (currentDisplayW * targetScale) / 2 + margin
        const targetY = screenH / 2 - (currentDisplayH * targetScale) / 2 - margin * 2

        const shrinkDuration = 500

        imageScale.value = withDelay(
          50,
          withTiming(targetScale, {
            duration: shrinkDuration,
            easing: Easing.bezier(0.25, 0.1, 0.25, 1),
          }),
        )
        imageTranslateX.value = withDelay(
          50,
          withTiming(targetX, {
            duration: shrinkDuration,
            easing: Easing.bezier(0.25, 0.1, 0.25, 1),
          }),
        )
        imageTranslateY.value = withDelay(
          50,
          withTiming(targetY, {
            duration: shrinkDuration,
            easing: Easing.bezier(0.25, 0.1, 0.25, 1),
          }),
        )

        // 4. Fade out
        imageOpacity.value = withDelay(
          50 + shrinkDuration + 500,
          withTiming(0, { duration: 300 }, (finished) => {
            if (finished) {
              runOnJS(setScreenshotUri)(null)
            }
          }),
        )
      }

      if (layout) {
        prepareAnimation(layout)
      } else {
        // Fallback: measure the image itself (if contentSize wasn't set)
        RNImage.getSize(
          uri,
          (imgW, imgH) => {
            const measuredAndCalculated = calculateLayout(imgW, imgH, screenW, screenH)
            prepareAnimation(measuredAndCalculated)
            // Optionally update contentSize? Maybe not, this is a fallback.
          },
          (err) => {
            console.error("Failed to measure screenshot", err)
          },
        )
      }
    },
  }))

  const flashStyle = useAnimatedStyle(() => ({
    opacity: flashOpacity.value,
  }))

  const imageStyle = useAnimatedStyle(() => ({
    opacity: imageOpacity.value,
    transform: [
      { translateX: imageTranslateX.value },
      { translateY: imageTranslateY.value },
      { scale: imageScale.value },
    ],
  }))

  return (
    <View style={StyleSheet.absoluteFill} pointerEvents="none">
      {/* Screenshot Image Layer */}
      {screenshotUri && (
        <Animated.View style={[StyleSheet.absoluteFill, styles.centered, imageStyle]}>
          <View
            style={[
              styles.imageContainer,
              layout
                ? { width: layout.width, height: layout.height }
                : { width: "100%", height: "100%" },
            ]}
          >
            <Image source={{ uri: screenshotUri }} style={styles.image} contentFit="contain" />
          </View>
        </Animated.View>
      )}

      {/* Flash Layer */}
      {layout ? (
        <Animated.View style={[StyleSheet.absoluteFill, styles.centered]}>
          <Animated.View
            style={[styles.flash, { width: layout.width, height: layout.height }, flashStyle]}
          />
        </Animated.View>
      ) : (
        <Animated.View style={[StyleSheet.absoluteFill, styles.flash, flashStyle]} />
      )}
    </View>
  )
})

const styles = StyleSheet.create({
  flash: {
    backgroundColor: "white",
  },
  centered: {
    justifyContent: "center",
    alignItems: "center",
  },
  imageContainer: {
    // Dynamic width/height now
    // backgroundColor: "black", // Removed to avoid black bars
    shadowColor: "#000",
    shadowOffset: {
      width: 0,
      height: 4,
    },
    shadowOpacity: 0.3,
    shadowRadius: 4.65,
    elevation: 8,
  },
  image: {
    flex: 1,
    borderRadius: 8, // Optional: rounded corners for the "card"
  },
})
