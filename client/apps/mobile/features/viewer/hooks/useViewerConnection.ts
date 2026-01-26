import { useState, useEffect, useRef, useCallback } from "react"
import {
  RTCPeerConnection,
  RTCIceCandidate,
  RTCSessionDescription,
  MediaStream,
} from "react-native-webrtc"

const SIGNALING_URL_BASE = "ws://10.0.2.2:3000/api/signal"

interface UseViewerConnectionProps {
  sessionId: string
  onDataChannelMessage?: (data: string | ArrayBuffer) => void
}

export function useViewerConnection({ sessionId, onDataChannelMessage }: UseViewerConnectionProps) {
  const [isConnected, setIsConnected] = useState(false)
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null)
  const [status, setStatus] = useState("Disconnected")

  const wsRef = useRef<WebSocket | null>(null)
  const pcRef = useRef<RTCPeerConnection | null>(null)
  const dcRef = useRef<any | null>(null)

  // Ref to hold the current callback so we don't need to rebuild connect function when it changes
  const onDataChannelMessageRef = useRef(onDataChannelMessage)
  useEffect(() => {
    onDataChannelMessageRef.current = onDataChannelMessage
  }, [onDataChannelMessage])

  const disconnect = useCallback(() => {
    if (wsRef.current) wsRef.current.close()
    if (pcRef.current) pcRef.current.close()
    if (dcRef.current) dcRef.current.close()
    wsRef.current = null
    pcRef.current = null
    dcRef.current = null
    setIsConnected(false)
    setRemoteStream(null)
    setStatus("Disconnected")
  }, [])

  // Cleanup on unmount
  useEffect(() => {
    return () => disconnect()
  }, [disconnect])

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
      // @ts-ignore
      pc.ontrack = (event: any) => {
        const track = event.track
        console.log("Track received:", track?.kind, track?.id)

        if (track && track.kind === "video") {
          track.enabled = true
          setRemoteStream((prev) => {
            const newStream = new MediaStream(undefined)
            newStream.addTrack(track)
            console.log(
              `Created new Video Stream: ${newStream.toURL()} with track ${track.id} (${track.kind}) state:${track.readyState}`,
            )
            return newStream
          })
        } else if (track) {
          console.log(`Ignoring non-video track for RTCView: ${track.kind} ${track.id}`)
          track.enabled = true
        }
      }

      // 3. Add Transceivers (RecvOnly)
      pc.addTransceiver("video", { direction: "recvonly" })
      pc.addTransceiver("audio", { direction: "recvonly" })

      // 4. Create Data Channel
      console.log("Creating DataChannel...")
      const dc = pc.createDataChannel("data")
      dcRef.current = dc

      // @ts-ignore
      dc.onopen = () => console.log("DataChannel Open")
      // @ts-ignore
      dc.onmessage = (event: any) => {
        if (onDataChannelMessageRef.current) {
          onDataChannelMessageRef.current(event.data)
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
            codec: "h264",
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
  }, [sessionId, disconnect])

  const sendDataChannelMessage = useCallback((msg: string) => {
    if (dcRef.current && dcRef.current.readyState === "open") {
      dcRef.current.send(msg)
    } else {
      console.warn("DataChannel not open")
    }
  }, [])

  return {
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    pcRef,
    sendDataChannelMessage,
  }
}
