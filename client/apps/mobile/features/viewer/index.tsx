import { useRouter } from "expo-router"
import { StatusBar } from "expo-status-bar"
import { useRef, useEffect, useState, useCallback } from "react"
import { View, StyleSheet, Platform } from "react-native"

import * as Pip from "@/modules/pip"

import { ScreenshotFlash, ScreenshotFlashHandle } from "./components/ScreenshotFlash"
import { VideoPlayer } from "./components/VideoPlayer"
import { ViewerOverlay } from "./components/ViewerOverlay"
import { useViewerContext } from "./context/ViewerContext"

export function ViewerScreen() {
  const flashRef = useRef<ScreenshotFlashHandle>(null)
  const router = useRouter()
  const [isInPip, setIsInPip] = useState(false)

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

  // PiP モード変更のリッスン (Android のみ)
  useEffect(() => {
    if (Platform.OS !== "android") return

    const sub = Pip.addPipModeListener((event) => {
      setIsInPip(event.isInPipMode)
    })
    return () => sub.remove()
  }, [])

  // ストリームがアクティブな時に自動 PiP を有効化 (Android のみ)
  useEffect(() => {
    if (Platform.OS !== "android") return

    if (isConnected && remoteStream) {
      Pip.setAutoEnterEnabled(true)
    }
    return () => {
      Pip.setAutoEnterEnabled(false)
    }
  }, [isConnected, remoteStream])

  // Navigation guard: PiP モード中はリダイレクトを抑制
  useEffect(() => {
    if (!isConnected && !isInPip) {
      router.replace("/")
    }
  }, [isConnected, isInPip, router])

  // Flash レイアウト更新
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

  // Flash トリガー
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

  // オーバーレイの自動非表示
  useEffect(() => {
    if (!showOverlay || !isConnected) return

    const timer = setTimeout(() => {
      setShowOverlay(false)
    }, 4000)

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
    return <View style={styles.container} />
  }

  return (
    <View style={styles.container}>
      {/* PiP モード中は StatusBar を非表示 */}
      <StatusBar hidden={(isConnected && !showOverlay) || isInPip} />
      <View style={styles.videoContainer}>
        <VideoPlayer stream={remoteStream} onTap={toggleOverlay} />
        {/* PiP モード中はオーバーレイを非表示 */}
        {!isInPip && (
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
        )}
      </View>
      {/* PiP モード中はフラッシュも非表示 */}
      {!isInPip && <ScreenshotFlash ref={flashRef} />}
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
