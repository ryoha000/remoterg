import * as ScreenOrientation from "expo-screen-orientation"
import { useState, useEffect, useCallback } from "react"

import { useScreenshot } from "./useScreenshot"
import { useViewerConnection } from "./useViewerConnection"
import { useViewerStats } from "./useViewerStats"

export function useViewer() {
  const [sessionId, setSessionId] = useState("fixed")

  const {
    requestScreenshot: requestScreenshotInternal,
    handleMetadata,
    handleChunk,
    isReceiving,
  } = useScreenshot()

  const handleDataChannelMessage = useCallback(
    (data: string | ArrayBuffer) => {
      if (typeof data === "string") {
        try {
          const msg = JSON.parse(data)
          if (msg.SCREENSHOT_METADATA) {
            handleMetadata(msg.SCREENSHOT_METADATA.payload)
          }
        } catch (e) {
          console.error("Failed to parse DC message", e)
        }
      } else {
        // Binary
        if (isReceiving()) {
          handleChunk(new Uint8Array(data))
        }
      }
    },
    [handleMetadata, handleChunk, isReceiving],
  )

  const { isConnected, remoteStream, status, connect, disconnect, pcRef, sendDataChannelMessage } =
    useViewerConnection({
      sessionId,
      onDataChannelMessage: handleDataChannelMessage,
    })

  useViewerStats(pcRef, isConnected)

  const requestScreenshot = useCallback(() => {
    requestScreenshotInternal(sendDataChannelMessage)
  }, [requestScreenshotInternal, sendDataChannelMessage])

  useEffect(() => {
    // Unlock orientation to allow user to rotate device nicely
    ScreenOrientation.unlockAsync()
  }, [])

  return {
    sessionId,
    setSessionId,
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    requestScreenshot,
  }
}
