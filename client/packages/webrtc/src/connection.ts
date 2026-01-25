import { Effect, Stream, Data } from "effect";

export class PeerConnectionError extends Data.TaggedError("PeerConnectionError")<{
  message: string;
  originalError?: unknown;
}> {}

export const makeConnection = (
  config: RTCConfiguration,
  createPeerConnection: (config: RTCConfiguration) => RTCPeerConnection = (c) =>
    new RTCPeerConnection(c),
) =>
  Effect.gen(function* () {
    const pc = yield* Effect.acquireRelease(
      Effect.sync(() => createPeerConnection(config)),
      (pc) => Effect.sync(() => pc.close()),
    );

    const connectionState = Stream.async<RTCPeerConnectionState>((emit) => {
      const handler = () => {
        void emit.single(pc.connectionState);
      };
      pc.addEventListener("connectionstatechange", handler);
      return Effect.sync(() => {
        pc.removeEventListener("connectionstatechange", handler);
      });
    });

    const iceConnectionState = Stream.async<RTCIceConnectionState>((emit) => {
      const handler = () => {
        void emit.single(pc.iceConnectionState);
      };
      pc.addEventListener("iceconnectionstatechange", handler);
      return Effect.sync(() => {
        pc.removeEventListener("iceconnectionstatechange", handler);
      });
    });

    const iceCandidates = Stream.async<RTCIceCandidate>((emit) => {
      const handler = (event: RTCPeerConnectionIceEvent) => {
        if (event.candidate) void emit.single(event.candidate);
      };
      pc.addEventListener("icecandidate", handler);
      return Effect.sync(() => {
        pc.removeEventListener("icecandidate", handler);
      });
    });

    const track = Stream.async<RTCTrackEvent>((emit) => {
      const handler = (event: RTCTrackEvent) => {
        void emit.single(event);
      };
      pc.addEventListener("track", handler);
      return Effect.sync(() => {
        pc.removeEventListener("track", handler);
      });
    });

    return { pc, connectionState, iceConnectionState, iceCandidates, track };
  });

export const setH264Preferences = (pc: RTCPeerConnection) =>
  Effect.sync(() => {
    try {
      const capabilities = RTCRtpSender.getCapabilities("video");
      const codecs = (capabilities?.codecs ?? []).filter(
        (c) =>
          c.mimeType === "video/H264" && (c.sdpFmtpLine ?? "").includes("packetization-mode=1"),
      );
      const transceiver = pc.getTransceivers().find((t) => t.receiver.track.kind === "video");
      if (
        codecs.length > 0 &&
        transceiver &&
        typeof transceiver.setCodecPreferences === "function"
      ) {
        transceiver.setCodecPreferences(codecs);
        return true;
      }
    } catch {
      return false;
    }
    return false;
  });
