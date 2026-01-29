import { useEffect, useRef, useState, RefObject } from "react"
import { RTCPeerConnection } from "react-native-webrtc"

export function useViewerStats(pcRef: RefObject<RTCPeerConnection | null>, isConnected: boolean) {
  const [stats, setStats] = useState({
    fps: 0,
    bitrate: 0,
    loss: 0,
  })

  useEffect(() => {
    if (!isConnected || !pcRef.current) return

    const pc = pcRef.current
    let lastBytesReceived = 0
    let lastTimestamp = 0

    const interval = setInterval(async () => {
      if (!pc) return

      try {
        // @ts-ignore
        const reports = await pc.getStats()
        // @ts-ignore
        reports.forEach((report) => {
          if (report.type === "inbound-rtp" && report.kind === "video") {
            const now = report.timestamp
            const bytes = report.bytesReceived

            if (lastTimestamp > 0) {
              const duration = (now - lastTimestamp) / 1000
              const bitrate = (bytes - lastBytesReceived) * 8 / duration / 1000 // kbps

              setStats({
                fps: report.framesPerSecond || 0,
                bitrate: Math.round(bitrate),
                loss: Math.round((report.packetsLost / report.packetsReceived) * 100) || 0,
              })
            }

            lastBytesReceived = bytes
            lastTimestamp = now
          }
        })
      } catch (e) {
        console.error("Stats logging error:", e)
      }
    }, 1000)

    return () => clearInterval(interval)
  }, [isConnected, pcRef])

  return stats
}
