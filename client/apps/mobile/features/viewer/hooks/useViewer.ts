import * as ScreenOrientation from "expo-screen-orientation"
import { useState, useEffect, useCallback, useRef } from "react"

import { useSaveAnalysis } from "@/db/queries/use-analysis"
import { getLocalIds } from "@/db/services/screenshot-service"

import { useScreenshot } from "./useScreenshot"
import { useViewerConnection } from "./useViewerConnection"
import { useViewerStats } from "./useViewerStats"

export function useViewer() {
  const [sessionId, setSessionId] = useState("fixed")
  const [analysisResults, setAnalysisResults] = useState<Record<string, any>>({})
  const [isAnalyzingMap, setIsAnalyzingMap] = useState<Record<string, boolean>>({})
  const [latestScreenshotUri, setLatestScreenshotUri] = useState<string | null>(null)

  const analysisBuffers = useRef<Record<string, string>>({})
  const { mutate: saveAnalysis } = useSaveAnalysis()

  const onAnalyzeResult = useCallback((id: string, result: any, isPartial: boolean) => {
    setAnalysisResults((prev) => ({ ...prev, [id]: result }))
    setIsAnalyzingMap((prev) => ({ ...prev, [id]: isPartial }))
  }, [])

  const {
    requestScreenshot: requestScreenshotInternal,
    handleMetadata,
    handleChunk,
    isReceiving,
  } = useScreenshot({
    onSuccess: (uri) => {
      setLatestScreenshotUri(uri)
    },
  })

  const handleDataChannelMessage = useCallback(
    (data: string | ArrayBuffer) => {
      if (typeof data === "string") {
        try {
          const msg = JSON.parse(data)
          if (msg.SCREENSHOT_METADATA) {
            handleMetadata(msg.SCREENSHOT_METADATA.payload)
          } else if (msg.ANALYZE_RESPONSE) {
            const result = JSON.parse(msg.ANALYZE_RESPONSE.text)

            // Resolve local IDs and save
            getLocalIds(msg.ANALYZE_RESPONSE.id).then((localIds) => {
              localIds.forEach((localId) => {
                saveAnalysis({ localId, analysis: result })
              })
            })

            onAnalyzeResult(msg.ANALYZE_RESPONSE.id, result, false)
          } else if (msg.ANALYZE_RESPONSE_CHUNK) {
            const { id, delta } = msg.ANALYZE_RESPONSE_CHUNK
            analysisBuffers.current[id] = (analysisBuffers.current[id] || "") + delta

            onAnalyzeResult(id, { raw: analysisBuffers.current[id] }, true)
          } else if (msg.ANALYZE_RESPONSE_DONE) {
            const id = msg.ANALYZE_RESPONSE_DONE.id
            const raw = analysisBuffers.current[id]
            if (raw) {
              try {
                const result = JSON.parse(raw)

                // Resolve local IDs and save
                getLocalIds(id).then((localIds) => {
                  localIds.forEach((localId) => {
                    saveAnalysis({ localId, analysis: result })
                  })
                })

                onAnalyzeResult(id, result, false)
              } catch (e) {
                console.error("Failed to parse final analysis", e)
              }
              delete analysisBuffers.current[id]
            }
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
    [handleMetadata, handleChunk, isReceiving, onAnalyzeResult, saveAnalysis],
  )

  const {
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    pcRef,
    sendDataChannelMessage,
    requestAnalyze: requestAnalyzeInternal,
  } = useViewerConnection({
    sessionId,
    onDataChannelMessage: handleDataChannelMessage,
  })

  const stats = useViewerStats(pcRef, isConnected)

  const requestScreenshot = useCallback(() => {
    requestScreenshotInternal(sendDataChannelMessage)
  }, [requestScreenshotInternal, sendDataChannelMessage])

  useEffect(() => {
    // Unlock orientation to allow user to rotate device nicely
    ScreenOrientation.unlockAsync()
  }, [])

  const requestAnalyze = useCallback(
    (id: string, max_edge?: number) => {
      requestAnalyzeInternal(id, max_edge)
    },
    [requestAnalyzeInternal],
  )

  return {
    sessionId,
    setSessionId,
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    requestScreenshot,
    stats,
    requestAnalyze,
    analysisResults,
    isAnalyzingMap,
    latestScreenshotUri,
  }
}
