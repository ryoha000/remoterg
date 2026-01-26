import { StatusBar } from "expo-status-bar"
import { useRef, useEffect, useState, useCallback } from "react"
import { View, StyleSheet, TouchableWithoutFeedback } from "react-native"

import { ConnectForm } from "./components/ConnectForm"
import { ScreenshotFlash, ScreenshotFlashHandle } from "./components/ScreenshotFlash"
import { VideoPlayer } from "./components/VideoPlayer"
import { ViewerOverlay } from "./components/ViewerOverlay"
import { useViewer } from "./hooks/useViewer"

export function ViewerScreen() {
  const flashRef = useRef<ScreenshotFlashHandle>(null)
  const {
    sessionId,
    setSessionId,
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    requestScreenshot: requestScreenshotInternal,
  } = useViewer({
    onScreenshotSuccess: (uri) => {
      flashRef.current?.showResult(uri)
    },
  })

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

  return (
    <View style={styles.container}>
      <StatusBar hidden={isConnected && !showOverlay} />
      {isConnected && remoteStream ? (
        <View style={styles.videoContainer}>
          <VideoPlayer stream={remoteStream} onTap={toggleOverlay} />
          <ViewerOverlay
            visible={showOverlay}
            status={status}
            onDisconnect={disconnect}
            sessionId={sessionId}
            stats={{
              fps: 60, // TODO: Get real stats
              bitrate: 0, // TODO: Get real stats
              loss: 0, // TODO: Get real stats
            }}
            onInteraction={onInteraction}
            onRequestScreenshot={requestScreenshot}
          />
        </View>
      ) : (
        <ConnectForm
          sessionId={sessionId}
          setSessionId={setSessionId}
          status={status}
          connect={connect}
        />
      )}
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
