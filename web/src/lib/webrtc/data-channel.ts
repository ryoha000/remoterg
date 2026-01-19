import { Effect, Queue, Schedule, Duration } from "effect";
import * as v from "valibot";

export const createDataChannel = (pc: RTCPeerConnection, label: string = "input") =>
  Effect.acquireRelease(
    Effect.sync(() => pc.createDataChannel(label, { ordered: true })),
    (dc) => Effect.sync(() => dc.close()),
  );

const ScreenshotMetadataPayloadSchema = v.object({
  id: v.string(),
  size: v.number(),
  format: v.string(),
});

const IncomingMessageSchema = v.object({
  SCREENSHOT_METADATA: v.optional(
    v.object({
      payload: ScreenshotMetadataPayloadSchema,
    }),
  ),
  ANALYZE_RESPONSE: v.optional(
    v.object({
      text: v.string(),
    }),
  ),
  Pong: v.optional(v.unknown()),
});

export const runDataChannel = (
  dc: RTCDataChannel,
  keyQ: Queue.Queue<{ key: string; down: boolean }>,
  screenQ: Queue.Queue<void>,
  analyzeQ: Queue.Queue<string>,
  onOpen: () => void,
  onScreenshot: (blob: Blob, meta: { id: string; format: string; size: number }) => void,
  onAnalyzeResult: (text: string) => void,
) =>
  Effect.gen(function* () {
    const waitForOpen = Effect.async<void>((resume) => {
      if (dc.readyState === "open") resume(Effect.void);
      else {
        const handler = () => {
          dc.removeEventListener("open", handler);
          resume(Effect.void);
        };
        dc.addEventListener("open", handler);
      }
    });

    const waitForCloseInitial = Effect.async<void>((resume) => {
      if (dc.readyState === "closed") resume(Effect.void);
      else {
        const handler = () => {
          dc.removeEventListener("close", handler);
          resume(Effect.void);
        };
        dc.addEventListener("close", handler);
      }
    });

    // Race to ensure we don't hang if it closes while waiting to open
    yield* Effect.race(waitForOpen, waitForCloseInitial);

    if (dc.readyState !== "open") {
      return;
    }

    onOpen();

    const keepAlive = Effect.repeat(
      Effect.sync(() => {
        if (dc.readyState === "open") {
          dc.send(JSON.stringify({ Ping: { timestamp: Date.now() } }));
        }
      }),
      Schedule.spaced(Duration.seconds(3)),
    );

    const processKeys = Queue.take(keyQ).pipe(
      Effect.tap((item) =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(JSON.stringify({ Key: { key: item.key, down: item.down } }));
          }
        }),
      ),
      Effect.forever,
    );

    const processScreens = Queue.take(screenQ).pipe(
      Effect.tap(() =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(JSON.stringify({ ScreenshotRequest: null }));
          }
        }),
      ),
      Effect.forever,
    );

    const processAnalyze = Queue.take(analyzeQ).pipe(
      Effect.tap((id) =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            // Send AnalyzeRequest with ID
            dc.send(JSON.stringify({ AnalyzeRequest: { id } }));
          }
        }),
      ),
      Effect.forever,
    );

    const waitForClose = Effect.async<void>((resume) => {
      const onClose = () => {
        dc.removeEventListener("close", onClose);
        dc.removeEventListener("error", onClose);
        dc.removeEventListener("message", onMessage);
        resume(Effect.void);
      };

      // Screenshot receiving state
      let incomingScreenshot: {
        id: string;
        size: number;
        format: string;
        received: number;
        chunks: Uint8Array[];
      } | null = null;

      const onMessage = (event: MessageEvent) => {
        if (typeof event.data === "string") {
          try {
            const raw = JSON.parse(event.data);
            const msg = v.parse(IncomingMessageSchema, raw);

            if (msg.SCREENSHOT_METADATA) {
              const payload = msg.SCREENSHOT_METADATA.payload;
              console.log("Screenshot metadata received:", payload);
              incomingScreenshot = {
                id: payload.id,
                size: payload.size,
                format: payload.format,
                received: 0,
                chunks: [],
              };
            } else if (msg.ANALYZE_RESPONSE) {
              console.log("Analysis response received");
              onAnalyzeResult(msg.ANALYZE_RESPONSE.text);
            } else if (msg.Pong) {
              // Handle Pong if needed
            }
          } catch (e) {
            console.error("Failed to parse data channel message:", e);
          }
        } else if (event.data instanceof ArrayBuffer) {
          if (incomingScreenshot) {
            const chunk = new Uint8Array(event.data);
            incomingScreenshot.chunks.push(chunk);
            incomingScreenshot.received += chunk.byteLength;

            if (incomingScreenshot.received >= incomingScreenshot.size) {
              console.log("Screenshot complete, creating blob");
              const blob = new Blob(incomingScreenshot.chunks as BlobPart[], {
                type: `image/${incomingScreenshot.format}`,
              });

              // Notify via callback instead of direct download
              onScreenshot(blob, {
                id: incomingScreenshot.id,
                format: incomingScreenshot.format,
                size: incomingScreenshot.size,
              });

              incomingScreenshot = null;
            }
          } else {
            console.warn("Received binary data without active screenshot metadata");
          }
        }
      };

      // Close and error both terminate the loop
      dc.addEventListener("close", onClose);
      dc.addEventListener("error", onClose);
      dc.addEventListener("message", onMessage);

      return Effect.sync(() => {
        dc.removeEventListener("close", onClose);
        dc.removeEventListener("error", onClose);
        dc.removeEventListener("message", onMessage);
      });
    });

    yield* Effect.race(
      Effect.all([keepAlive, processKeys, processScreens, processAnalyze], {
        concurrency: "unbounded",
      }),
      waitForClose,
    );
  });
