import { useEffect, RefObject } from "react"
import { RTCPeerConnection } from "react-native-webrtc"

export function useViewerStats(pcRef: RefObject<RTCPeerConnection | null>, isConnected: boolean) {
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
  }, [isConnected, pcRef])
}
