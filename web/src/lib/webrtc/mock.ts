import { Effect, Duration } from "effect";
import { PeerConnectionError } from "./connection";

// Helper interface for video element with non-standard captureStream
interface VideoElementWithCapture extends HTMLVideoElement {
  captureStream?(): MediaStream;
  mozCaptureStream?(): MediaStream;
}

export const runMockMode = (
  onTrack: ((stream: MediaStream) => void) | undefined,
  setConnectionState: (state: string) => void,
  setIceConnectionState: (state: string) => void,
  log: (msg: string, type?: string) => void,
) =>
  Effect.gen(function* () {
    log("Mock Mode: Starting sequence...", "info");

    yield* Effect.sync(() => setConnectionState("connecting"));

    const cleanupVideo = yield* Effect.acquireRelease(
      Effect.sync(() => {
        const vid = document.createElement("video");
        vid.src = "/mock.mp4";
        vid.loop = true;
        vid.muted = true;
        vid.playsInline = true;
        vid.style.position = "absolute";
        vid.style.top = "-9999px";
        vid.style.left = "-9999px";
        document.body.appendChild(vid);
        return vid;
      }),
      (vid) =>
        Effect.sync(() => {
          vid.pause();
          vid.remove();
        }),
    );

    yield* Effect.promise(() => cleanupVideo.play());
    log("Mock Mode: Video playing", "success");

    const stream = yield* Effect.try({
      try: () => {
        const videoElement = cleanupVideo as VideoElementWithCapture;
        if (typeof videoElement.captureStream === "function") {
          return videoElement.captureStream();
        } else if (typeof videoElement.mozCaptureStream === "function") {
          return videoElement.mozCaptureStream();
        }
        throw new Error("captureStream not supported");
      },
      catch: (e) => new PeerConnectionError({ message: String(e) }),
    });

    yield* Effect.sleep(Duration.millis(1000));

    yield* Effect.sync(() => {
      setConnectionState("connected");
      setIceConnectionState("connected");
      onTrack?.(stream);
      log("Mock Mode: Connected", "success");
    });

    yield* Effect.never;
  });
