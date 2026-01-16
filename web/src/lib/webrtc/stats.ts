import { Effect, Schedule, Duration } from "effect";

export interface WebRTCStats {
  inbound?: {
    bytesReceived?: number;
    framesReceived?: number;
    packetsLost?: number;
  };
  track?: {
    framesDecoded?: number;
    framesDropped?: number;
    freezeCount?: number;
  };
}

export const runStatsLoop = (pc: RTCPeerConnection, onStats: (stats: WebRTCStats) => void) =>
  Effect.repeat(
    Effect.promise(async () => {
      const receiver = pc.getReceivers().find((r) => r.track?.kind === "video");
      if (!receiver) return;

      const reports = await receiver.getStats();
      let inbound: WebRTCStats["inbound"];
      let track: WebRTCStats["track"];

      reports.forEach((report) => {
        if (report.type === "inbound-rtp" && report.kind === "video") {
          inbound = {
            bytesReceived: report.bytesReceived,
            framesReceived: report.framesReceived,
            packetsLost: report.packetsLost,
          };
        } else if (report.type === "track" && report.kind === "video") {
          track = {
            framesDecoded: report.framesDecoded,
            framesDropped: report.framesDropped,
            freezeCount: report.freezeCount,
          };
        }
      });

      onStats({ inbound, track });
    }),
    Schedule.spaced(Duration.seconds(2)),
  );
