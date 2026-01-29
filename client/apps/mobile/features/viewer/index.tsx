import { useRouter } from "expo-router"
import { StatusBar } from "expo-status-bar"
import { useRef, useEffect, useState, useCallback } from "react"
import { View, StyleSheet } from "react-native"

import { ScreenshotFlash, ScreenshotFlashHandle } from "./components/ScreenshotFlash"
import { VideoPlayer } from "./components/VideoPlayer"
import { ViewerOverlay } from "./components/ViewerOverlay"
import { useViewerContext } from "./context/ViewerContext"

export function ViewerScreen() {
  const flashRef = useRef<ScreenshotFlashHandle>(null)
  const router = useRouter()
  
  const {
    sessionId,
    isConnected,
    remoteStream,
    status,
    disconnect,
    requestScreenshot: requestScreenshotInternal,
    stats,
    requestAnalyze,
    analysisResults,
    isAnalyzingMap,
    latestScreenshotUri,
  } = useViewerContext()

  // Navigation guard
  useEffect(() => {
    if (!isConnected) {
      router.replace("/")
    }
  }, [isConnected, router])

  // Update flash layout when stream dimensions change
  useEffect(() => {
    if (remoteStream) {
      const track = remoteStream.getVideoTracks()[0]
      if (track) {
        const { width, height } = track.getSettings()
        if (width && height) {
          flashRef.current?.setContentSize(width, height)
        }
      }
    }
  }, [remoteStream])

  // Flash trigger
  useEffect(() => {
    if (latestScreenshotUri) {
      flashRef.current?.showResult(latestScreenshotUri)
    }
  }, [latestScreenshotUri])

  const requestScreenshot = useCallback(() => {
    flashRef.current?.triggerFlash()
    requestScreenshotInternal()
  }, [requestScreenshotInternal])

  const [showOverlay, setShowOverlay] = useState(true)
  const [lastInteraction, setLastInteraction] = useState(Date.now())

  // Auto-hide overlay
  useEffect(() => {
    if (!showOverlay || !isConnected) return

    const timer = setTimeout(() => {
      setShowOverlay(false)
    }, 4000) // Hide after 4 seconds

    return () => clearTimeout(timer)
  }, [showOverlay, isConnected, lastInteraction])

  const toggleOverlay = useCallback(() => {
    setShowOverlay((prev) => !prev)
    setLastInteraction(Date.now())
  }, [])

  const onInteraction = useCallback(() => {
    setLastInteraction(Date.now())
  }, [])

  if (!isConnected || !remoteStream) {
      // Should handle by navigation guard, but for safety return null or loading
      return <View style={styles.container} />
  }

  return (
    <View style={styles.container}>
      <StatusBar hidden={isConnected && !showOverlay} />
      <View style={styles.videoContainer}>
        <VideoPlayer stream={remoteStream} onTap={toggleOverlay} />
        <ViewerOverlay
          visible={showOverlay}
          status={status}
          onDisconnect={disconnect}
          sessionId={sessionId}
          stats={stats}
          onInteraction={onInteraction}
          onRequestScreenshot={requestScreenshot}
          onRequestAnalyze={requestAnalyze}
          analysisResults={analysisResults}
          isAnalyzingMap={isAnalyzingMap}
        />
      </View>
      <ScreenshotFlash ref={flashRef} />
    </View>
  )
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: "#000",
  },
  videoContainer: {
    flex: 1,
    justifyContent: "center",
    width: "100%",
    height: "100%",
  },
})
