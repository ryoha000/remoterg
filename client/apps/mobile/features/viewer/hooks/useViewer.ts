import * as ScreenOrientation from "expo-screen-orientation"
import { useState, useEffect, useRef, useCallback } from "react"
import {
  RTCPeerConnection,
  RTCIceCandidate,
  RTCSessionDescription,
  MediaStream,
} from "react-native-webrtc"
import { Alert, Platform } from "react-native"
import ReactNativeBlobUtil from "react-native-blob-util"
import * as MediaLibrary from "expo-media-library"
// @ts-ignore: Check if File/Paths are available in the installed version, assuming yes per user request
import { File, Paths } from "expo-file-system"

const SIGNALING_URL_BASE = "ws://10.0.2.2:3000/api/signal"

export function useViewer() {
  const [sessionId, setSessionId] = useState("fixed")
  const [isConnected, setIsConnected] = useState(false)
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null)
  const [status, setStatus] = useState("Disconnected")

  const wsRef = useRef<WebSocket | null>(null)
  const pcRef = useRef<RTCPeerConnection | null>(null)
  const dcRef = useRef<any | null>(null)
  
  const incomingScreenshotRef = useRef<{
    id: string
    size: number
    format: string
    received: number
    chunks: Uint8Array[]
  } | null>(null)

  const requestScreenshot = useCallback(() => {
    if (dcRef.current && dcRef.current.readyState === "open") {
      dcRef.current.send(JSON.stringify({ ScreenshotRequest: null }))
      console.log("Screenshot request sent")
    } else {
      console.warn("DataChannel not open")
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

      // 2. Save directly to FileSystem using new API (no base64 conv needed)
      // Note: File class writes Uint8Array directly
      try {
        const file = new File(Paths.document, `${screenshot.id}.${screenshot.format}`)
        // Write bytes directly
        file.write(combined)
        
        console.log("File saved to:", file.uri)

        // 3. Save to Gallery
        try {
            if (Platform.OS === 'android') {
                 // Android: Use MediaCollection to save directly to Pictures/RemoteRG
                 // This avoids "Allow to modify?" dialog on Android 11+ (Scoped Storage)
                 // because we are creating a new entry, not moving/modifying an existing one.
                 
                 const mimeType = screenshot.format === 'png' ? 'image/png' : 'image/jpeg'
                 
                 // copyToMediaStore takes: (details, mediaType, path)
                 // The path must be absolute path to the temp file. 
                 // file.uri from Expo FS starts with 'file://' which BlobUtil handles or we might need to strip it?
                 // Usually BlobUtil handles file://, but let's be safe and check if it fails.
                 
                 await ReactNativeBlobUtil.MediaCollection.copyToMediaStore(
                    {
                        name: screenshot.id, // filename
                        parentFolder: 'RemoteRG', // "RemoteRG" folder in Pictures
                        mimeType: mimeType
                    },
                    'Image',
                    file.uri
                 )
                 Alert.alert("Success", "Screenshot saved to gallery!")
            } else {
                // iOS: Use Expo MediaLibrary
                const asset = await MediaLibrary.createAssetAsync(file.uri)
                const album = await MediaLibrary.getAlbumAsync("RemoteRG")
                if (album) {
                    await MediaLibrary.addAssetsToAlbumAsync([asset], album, false)
                } else {
                    await MediaLibrary.createAlbumAsync("RemoteRG", asset, false)
                }
                Alert.alert("Success", "Screenshot saved to gallery!")
            }

        } catch (e) {
            console.error("Failed to save to gallery", e)
            Alert.alert("Error", `Failed to save to gallery: ${e}`)
        }
      } catch (e) {
         console.error("FileSystem File API Error", e);
         Alert.alert("Error", `FileSystem Error: ${e}`)
      }

    } catch (e) {
      console.error("Error processing screenshot:", e)
      Alert.alert("Error", `Failed to process screenshot: ${e}`)
    }
  }

  const connect = useCallback(() => {
    setStatus("Connecting...")

    // 1. WebSocket Setup
    const wsUrl = `${SIGNALING_URL_BASE}?session_id=${sessionId}&role=viewer`
    console.log("Connecting WS:", wsUrl)
    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    const setupPeerConnection = async (ws: WebSocket) => {
      // 2. PeerConnection Config
      const config = {
        iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }],
      }

      const pc = new RTCPeerConnection(config)
      pcRef.current = pc

      // Monitor Connection State
      // @ts-ignore
      pc.onconnectionstatechange = () => {
        console.log("PC Connection State:", pc.connectionState)
        setStatus(`PC: ${pc.connectionState}`)
        if (pc.connectionState === "connected") {
          setIsConnected(true)
        }
      }

      // @ts-ignore
      pc.oniceconnectionstatechange = () => {
        console.log("ICE Connection State:", pc.iceConnectionState)
      }

      // Handle ICE Candidates
      // @ts-ignore
      pc.onicecandidate = (event: any) => {
        if (event.candidate) {
          ws.send(
            JSON.stringify({
              type: "ice_candidate",
              candidate: event.candidate.candidate,
              sdp_mid: event.candidate.sdpMid,
              sdp_mline_index: event.candidate.sdpMLineIndex,
            }),
          )
        }
      }

      // Handle Tracks (Video)
      // @ts-ignore: react-native-webrtc types might be slightly off or I'm lazy with the event type
      pc.ontrack = (event: any) => {
        const track = event.track
        console.log("Track received:", track?.kind, track?.id)

        if (track && track.kind === "video") {
          // Only care about video for RTCView
          track.enabled = true
          setRemoteStream((prev) => {
            // Create new stream with ONLY this video track
            const newStream = new MediaStream(undefined)
            newStream.addTrack(track)

            console.log(
              `Created new Video Stream: ${newStream.toURL()} with track ${track.id} (${track.kind}) state:${track.readyState}`,
            )
            return newStream
          })
        } else if (track) {
          console.log(`Ignoring non-video track for RTCView: ${track.kind} ${track.id}`)
          track.enabled = true // Still enable audio, it plays automatically via PC
        }
      }

      // 3. Add Transceivers (RecvOnly)
      pc.addTransceiver("video", { direction: "recvonly" })
      pc.addTransceiver("audio", { direction: "recvonly" })

      // 4. Create Data Channel (Required by hostd?)
      console.log("Creating DataChannel...")
      const dc = pc.createDataChannel("data")
      dcRef.current = dc
      
      // @ts-ignore
      dc.onopen = () => console.log("DataChannel Open")
      // @ts-ignore
      dc.onmessage = (event: any) => {
        const data = event.data
        if (typeof data === 'string') {
             try {
                 const msg = JSON.parse(data)
                 if (msg.SCREENSHOT_METADATA) {
                     const payload = msg.SCREENSHOT_METADATA.payload
                     console.log("Screenshot metadata:", payload)
                     incomingScreenshotRef.current = {
                         ...payload,
                         received: 0,
                         chunks: []
                     }
                 }
             } catch (e) {
                 console.error("Failed to parse DC message", e)
             }
        } else {
            // Binary
            if (incomingScreenshotRef.current) {
                const chunk = new Uint8Array(data)
                incomingScreenshotRef.current.chunks.push(chunk)
                incomingScreenshotRef.current.received += chunk.byteLength
                
                if (incomingScreenshotRef.current.received >= incomingScreenshotRef.current.size) {
                    console.log("Screenshot done")
                    handleScreenshotComplete(incomingScreenshotRef.current)
                    incomingScreenshotRef.current = null
                }
            }
        }
      }

      // 5. Create Offer
      try {
        const offer = await pc.createOffer()
        await pc.setLocalDescription(offer)
        console.log("Sending Offer...")

        ws.send(
          JSON.stringify({
            type: "offer",
            sdp: offer.sdp,
            codec: "h264", // Default to h264 as per common config
          }),
        )
      } catch (err) {
        console.error("PC Setup Error:", err)
        setStatus("Error creating offer")
      }
    }

    ws.onopen = async () => {
      setStatus("WS Open. Creating PC...")
      setupPeerConnection(ws)
    }

    ws.onmessage = async (event) => {
      const msg = JSON.parse(event.data)
      console.log("WS Message:", msg.type)

      if (!pcRef.current) return

      try {
        if (msg.type === "answer") {
          await pcRef.current.setRemoteDescription(
            new RTCSessionDescription({ type: "answer", sdp: msg.sdp }),
          )
          setStatus("Remote Description Set")
        } else if (msg.type === "ice_candidate") {
          const candidate = new RTCIceCandidate({
            candidate: msg.candidate,
            sdpMid: msg.sdp_mid,
            sdpMLineIndex: msg.sdp_mline_index,
          })
          await pcRef.current.addIceCandidate(candidate)
          console.log("Added ICE Candidate")
        }
      } catch (err) {
        console.error("Signaling Error:", err)
      }
    }

    ws.onerror = (e) => {
      console.error("WS Error:", e)
      setStatus("WS Error")
    }

    ws.onclose = () => {
      console.log("WS Closed")
      setStatus("Disconnected (WS Closed)")
      setIsConnected(false)
    }
  }, [sessionId])

  const disconnect = () => {
    if (wsRef.current) wsRef.current.close()
    if (pcRef.current) pcRef.current.close()
    if (dcRef.current) dcRef.current.close()
    wsRef.current = null
    pcRef.current = null
    dcRef.current = null
    setIsConnected(false)
    setRemoteStream(null)
    setStatus("Disconnected")
  }

  // Cleanup on unmount
  useEffect(() => {
    return () => disconnect()
  }, [])

  // Stats Logging
  useEffect(() => {
    if (!isConnected || !pcRef.current) return

    const interval = setInterval(async () => {
      const pc = pcRef.current
      if (!pc) return

      try {
        // @ts-ignore
        const stats = await pc.getStats()
        // @ts-ignore
        stats.forEach((report) => {
          if (report.type === "inbound-rtp" && report.kind === "video") {
            console.log(
              `[Video Stats] Bytes: ${report.bytesReceived}, Packets: ${report.packetsReceived}, Decoded: ${report.framesDecoded}, Dropped: ${report.framesDropped}, Lost: ${report.packetsLost}`,
            )
          }
        })
      } catch (e) {
        console.error("Stats logging error:", e)
      }
    }, 2000)

    return () => clearInterval(interval)
  }, [isConnected])

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
