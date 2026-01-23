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

const MouseClickSchema = v.object({
  x: v.number(),
  y: v.number(),
  button: v.string(),
});

export const LlmConfigSchema = v.object({
  port: v.number(),
  model_path: v.nullable(v.string()),
  mmproj_path: v.nullable(v.string()),
});

export type LlmConfig = v.InferOutput<typeof LlmConfigSchema>;

const IncomingMessageSchema = v.object({
  SCREENSHOT_METADATA: v.optional(
    v.object({
      payload: ScreenshotMetadataPayloadSchema,
    }),
  ),
  ANALYZE_RESPONSE: v.optional(
    v.object({
      id: v.string(),
      text: v.string(),
    }),
  ),
  ANALYZE_RESPONSE_CHUNK: v.optional(
    v.object({
      id: v.string(),
      delta: v.string(),
    }),
  ),
  ANALYZE_RESPONSE_DONE: v.optional(
    v.object({
      id: v.string(),
    }),
  ),
  Pong: v.optional(v.unknown()),
  LlmConfigResponse: v.optional(
    v.object({
      config: LlmConfigSchema,
    }),
  ),
});

export const runDataChannel = (
  dc: RTCDataChannel,
  keyQ: Queue.Queue<{ key: string; down: boolean }>,
  screenQ: Queue.Queue<void>,
  analyzeQ: Queue.Queue<{ id: string; max_edge: number }>,
  mouseClickQ: Queue.Queue<{ x: number; y: number; button: string }>,
  onOpen: () => void,
  onScreenshot: (blob: Blob, meta: { id: string; format: string; size: number }) => void,
  onAnalyzeResult: (id: string, text: string) => void,
  onAnalyzeResultDelta: (id: string, delta: string) => void,
  onAnalyzeDone: (id: string) => void,
  getLlmConfigQ: Queue.Queue<void>,
  updateLlmConfigQ: Queue.Queue<LlmConfig>,
  onLlmConfig: (config: LlmConfig) => void,
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

    const processMouseClick = Queue.take(mouseClickQ).pipe(
      Effect.tap((click) =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(
              JSON.stringify({
                MouseClick: { x: click.x, y: click.y, button: click.button },
              }),
            );
          }
        }),
      ),
      Effect.forever,
    );

    const processAnalyze = Queue.take(analyzeQ).pipe(
      Effect.tap((payload) =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            // Send AnalyzeRequest with ID and max_edge
            dc.send(
              JSON.stringify({
                AnalyzeRequest: { id: payload.id, max_edge: payload.max_edge },
              }),
            );
          }
        }),
      ),
      Effect.forever,
    );

    const processGetLlmConfig = Queue.take(getLlmConfigQ).pipe(
      Effect.tap(() =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(JSON.stringify({ GetLlmConfig: null }));
          }
        }),
      ),
      Effect.forever,
    );

    const processUpdateLlmConfig = Queue.take(updateLlmConfigQ).pipe(
      Effect.tap((config) =>
        Effect.sync(() => {
          if (dc.readyState === "open") {
            dc.send(JSON.stringify({ UpdateLlmConfig: { config } }));
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
              console.log("Analysis response received");
              onAnalyzeResult(msg.ANALYZE_RESPONSE.id, msg.ANALYZE_RESPONSE.text);
            } else if (msg.ANALYZE_RESPONSE_CHUNK) {
               // console.log("Analysis chunk received");
               onAnalyzeResultDelta(
                 msg.ANALYZE_RESPONSE_CHUNK.id,
                 msg.ANALYZE_RESPONSE_CHUNK.delta,
               );
            } else if (msg.ANALYZE_RESPONSE_DONE) {
               console.log("Analysis done received");
               onAnalyzeDone(msg.ANALYZE_RESPONSE_DONE.id);
            } else if (msg.LlmConfigResponse) {
              console.log("LlmConfig received:", msg.LlmConfigResponse.config);
              onLlmConfig(msg.LlmConfigResponse.config);
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
      Effect.all(
        [
          keepAlive,
          processKeys,
          processScreens,
          processAnalyze,
          processMouseClick,
          processGetLlmConfig,
          processUpdateLlmConfig,
        ],
        {
          concurrency: "unbounded",
        },
      ),
      waitForClose,
    );
  });
