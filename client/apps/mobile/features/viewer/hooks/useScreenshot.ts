// @ts-ignore: Check if File/Paths are available in the installed version, assuming yes per user request
import { File, Paths } from "expo-file-system"
import * as MediaLibrary from "expo-media-library"
import { useState, useRef, useCallback } from "react"
import { Alert, Platform } from "react-native"
import ReactNativeBlobUtil from "react-native-blob-util"

import { mapScreenshot } from "@/lib/db"

interface ScreenshotMetadata {
  id: string
  size: number
  format: string
  received: number
  chunks: Uint8Array[]
}

interface UseScreenshotProps {
  onSuccess?: (uri: string) => void
}

export function useScreenshot({ onSuccess }: UseScreenshotProps = {}) {
  const incomingScreenshotRef = useRef<ScreenshotMetadata | null>(null)

  const requestScreenshot = useCallback((sendDataChannelMessage?: (msg: string) => void) => {
    if (sendDataChannelMessage) {
      sendDataChannelMessage(JSON.stringify({ ScreenshotRequest: null }))
      console.log("Screenshot request sent")
    } else {
      console.warn("DataChannel not available for screenshot request")
    }
  }, [])

  const handleScreenshotComplete = async (screenshot: {
    id: string
    format: string
    chunks: Uint8Array[]
  }) => {
    try {
      // 1. Combine chunks
      let totalLength = 0
      for (const chunk of screenshot.chunks) {
        totalLength += chunk.length
      }
      const combined = new Uint8Array(totalLength)
      let offset = 0
      for (const chunk of screenshot.chunks) {
        combined.set(chunk, offset)
        offset += chunk.length
      }

      // 2. Save directly to FileSystem using new API
      try {
        const file = new File(Paths.document, `${screenshot.id}.${screenshot.format}`)
        // Write bytes directly
        file.write(combined)

        console.log("File saved to:", file.uri)

        // 3. Save to Gallery
        try {
          if (Platform.OS === "android") {
            const mimeType = screenshot.format === "png" ? "image/png" : "image/jpeg"

            await ReactNativeBlobUtil.MediaCollection.copyToMediaStore(
              {
                name: screenshot.id,
                parentFolder: "RemoteRG",
                mimeType: mimeType,
              },
              "Image",
              file.uri,
            )
            onSuccess?.(file.uri)
          } else {
            const asset = await MediaLibrary.createAssetAsync(file.uri)
            // Map local ID to host ID
            await mapScreenshot(asset.id, screenshot.id)

            const album = await MediaLibrary.getAlbumAsync("RemoteRG")
            if (album) {
              await MediaLibrary.addAssetsToAlbumAsync([asset], album, false)
            } else {
              await MediaLibrary.createAlbumAsync("RemoteRG", asset, false)
            }
            onSuccess?.(file.uri)
          }
        } catch (e) {
          console.error("Failed to save to gallery", e)
          Alert.alert("Error", `Failed to save to gallery: ${e}`)
        }
      } catch (e) {
        console.error("FileSystem File API Error", e)
        Alert.alert("Error", `FileSystem Error: ${e}`)
      }
    } catch (e) {
      console.error("Error processing screenshot:", e)
      Alert.alert("Error", `Failed to process screenshot: ${e}`)
    }
  }

  const handleMetadata = useCallback((metadata: any) => {
    console.log("Screenshot metadata:", metadata)
    incomingScreenshotRef.current = {
      ...metadata,
      received: 0,
      chunks: [],
    }
  }, [])

  const handleChunk = useCallback((chunk: Uint8Array) => {
    if (incomingScreenshotRef.current) {
      incomingScreenshotRef.current.chunks.push(chunk)
      incomingScreenshotRef.current.received += chunk.byteLength

      if (incomingScreenshotRef.current.received >= incomingScreenshotRef.current.size) {
        console.log("Screenshot done")
        handleScreenshotComplete(incomingScreenshotRef.current)
        incomingScreenshotRef.current = null
      }
    }
  }, [])

  const isReceiving = useCallback(() => {
    return incomingScreenshotRef.current !== null
  }, [])

  return {
    requestScreenshot,
    handleMetadata,
    handleChunk,
    isReceiving,
  }
}
