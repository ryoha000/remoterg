import { StatusBar } from "expo-status-bar"
import { useRef, useEffect, useState, useCallback } from "react"
import { View, StyleSheet, TouchableWithoutFeedback } from "react-native"

import { ConnectForm } from "./components/ConnectForm"
import { VideoPlayer } from "./components/VideoPlayer"
import { ViewerOverlay } from "./components/ViewerOverlay"
import { useViewer } from "./hooks/useViewer"

export function ViewerScreen() {
  const {
    sessionId,
    setSessionId,
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    rotate,
  } = useViewer()

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
            onRotate={rotate}
            sessionId={sessionId}
            stats={{
              fps: 60, // TODO: Get real stats
              bitrate: 0, // TODO: Get real stats
              loss: 0, // TODO: Get real stats
            }}
            onInteraction={onInteraction}
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
