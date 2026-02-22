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

  const [layout, setLayout] = useState({ x: 0, y: 0, width: 0, height: 0 })
  const videoContainerRef = useRef<View>(null)

  // PiP モード変更のリッスン (Android のみ)
  useEffect(() => {
    if (Platform.OS !== "android") return

    const sub = Pip.addPipModeListener((event) => {
      setIsInPip(event.isInPipMode)
    })
    return () => sub.remove()
  }, [])

  // PiP パラメータの計算と設定
  const updatePipParams = useCallback(
    (enabled: boolean) => {
      if (Platform.OS !== "android") return
      if (!remoteStream) {
        Pip.setAutoEnterEnabled(enabled)
        return
      }

      const track = remoteStream.getVideoTracks()[0]
      const settings = track?.getSettings()
      const videoWidth = settings?.width ?? 0
      const videoHeight = settings?.height ?? 0

      if (videoWidth > 0 && videoHeight > 0 && layout.width > 0 && layout.height > 0) {
        // コンテナ内でのビデオの表示サイズを計算 (objectFit: contain)
        const widthRatio = layout.width / videoWidth
        const heightRatio = layout.height / videoHeight
        const scale = Math.min(widthRatio, heightRatio)

        const displayWidth = videoWidth * scale
        const displayHeight = videoHeight * scale

        // 画面上での絶対座標を計算（コンテナがセンタリングしている前提）
        // layout.x/y は親に対する相対座標だが、ルートに近いので簡易的に使うか、measureInWindowを使うべき。
        // ここでは簡易的に、VideoPlayerが画面全体を使っていると仮定して、センタリングのオフセットのみ計算する。
        // 厳密には videoContainerRef.current.measureInWindow が必要だが、非同期になるため
        // レイアウト変更時に measureInWindow を呼ぶようにする。

        // 一旦、簡易計算：コンテナの中心にビデオがある
        const offsetX = (layout.width - displayWidth) / 2
        const offsetY = (layout.height - displayHeight) / 2

        // 絶対座標（layout.x/y がウィンドウ基準と仮定... できない場合は measureInWindow の値を使う必要がある）
        // layout ステートに絶対座標を入れるように onLayout を修正するのが良いが、
        // onLayout の event.nativeEvent.layout は親相対。
        
        // 暫定：videoContainerRefから取得した絶対座標を使う
        videoContainerRef.current?.measureInWindow((x, y, w, h) => {
             const finalX = x + offsetX
             const finalY = y + offsetY
             
             if (enabled) {
                 Pip.setAutoEnterEnabled(true, videoWidth, videoHeight, Math.floor(finalX), Math.floor(finalY), Math.floor(displayWidth), Math.floor(displayHeight))
             } else {
                 Pip.setPipParams(videoWidth, videoHeight, Math.floor(finalX), Math.floor(finalY), Math.floor(displayWidth), Math.floor(displayHeight))
             }
        })
      } else {
         Pip.setAutoEnterEnabled(enabled)
      }
    },
    [remoteStream, layout],
  )

  // ストリームやレイアウトが変わったら PiP パラメータを更新
  useEffect(() => {
    if (isConnected && remoteStream) {
        updatePipParams(true)
    }
    return () => {
        // ここで false にすると画面遷移時に無効化される
    }
  }, [isConnected, remoteStream, updatePipParams])

  // クリーンアップ
  useEffect(() => {
     return () => {
         if (Platform.OS === "android") {
             Pip.setAutoEnterEnabled(false)
         }
     }
  }, [])

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
      <View 
        ref={videoContainerRef}
        style={styles.videoContainer}
        onLayout={() => {
            // レイアウト変更時に再計算をトリガーしたいが、measureInWindow は非同期。
            // ここでは単に updatePipParams を呼べるように state を更新するか、
            // 直接 measureInWindow して updatePipParams を呼ぶ。
            videoContainerRef.current?.measureInWindow((x, y, w, h) => {
                setLayout({ x, y, width: w, height: h })
            })
        }}
      >
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
