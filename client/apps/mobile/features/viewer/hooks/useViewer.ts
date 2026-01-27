import * as ScreenOrientation from "expo-screen-orientation"
import { useState, useEffect, useCallback, useRef } from "react"

import { useSaveAnalysis } from "@/db/queries/use-analysis"
import { getLocalIds } from "@/db/services/screenshot-service"

import { useScreenshot } from "./useScreenshot"
import { useViewerConnection } from "./useViewerConnection"
import { useViewerStats } from "./useViewerStats"

interface UseViewerProps {
  onScreenshotSuccess?: (uri: string) => void
  onAnalyzeResult?: (id: string, result: any, isPartial: boolean) => void
}

export function useViewer(props?: UseViewerProps) {
  const [sessionId, setSessionId] = useState("fixed")
  const analysisBuffers = useRef<Record<string, string>>({})
  const { mutate: saveAnalysis } = useSaveAnalysis()

  const {
    requestScreenshot: requestScreenshotInternal,
    handleMetadata,
    handleChunk,
    isReceiving,
  } = useScreenshot({ onSuccess: props?.onScreenshotSuccess })

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
                // We might need to notify UI with Host ID because UI subscribes to Host ID or Local ID?
                // Currently UI logic uses Host ID (from filename) to subscribe.
                // But if we want to support multiple local copies of same host ID?
                // Actually, if we use Host ID for everything in memory, it's easier.
                // But DB saves by Local ID.
              })
            })

            props?.onAnalyzeResult?.(msg.ANALYZE_RESPONSE.id, result, false)
          } else if (msg.ANALYZE_RESPONSE_CHUNK) {
            const { id, delta } = msg.ANALYZE_RESPONSE_CHUNK
            analysisBuffers.current[id] = (analysisBuffers.current[id] || "") + delta

            // Try to parse partial result or just send raw text if the UI handles stream
            // The UI expects an object usually.
            // For now let's just send the raw text if parsing fails?
            // Or better, let's try to parse if possible, or just ignore until done?
            // Web implementation updates live.
            // Let's try to parse "best effort" or just send null if invalid?
            // Actually, we can just pass the raw buffer to the UI if it wants to show "Generating..."
            // But we specifically need structured data (Scene, Dialogue).
            // Let's rely on valid JSON chunks or just wait for complete for now?
            // No, user wants "AI functionality like web". Web shows streaming.
            // Web uses `best-effort-json-parser`. I don't have it here.
            // I'll stick to full response for structure, but maybe just notify "generating" state?
            // Actually, I can allow the UI to receive the raw text buffer too.
            // Let's pass the buffer as a rawAnalysisText field or something?
            // For now, let's keep it simple: notify that there IS an update.
            props?.onAnalyzeResult?.(id, { raw: analysisBuffers.current[id] }, true)
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

                props?.onAnalyzeResult?.(id, result, false)
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
    [handleMetadata, handleChunk, isReceiving, props?.onAnalyzeResult],
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
  }
}
