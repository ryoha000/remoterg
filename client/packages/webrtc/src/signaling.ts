import { Effect, Stream, Data } from "effect";
import * as v from "valibot";

export class WebSocketError extends Data.TaggedError("WebSocketError")<{
  message: string;
  originalError?: unknown;
}> {}

const ErrorMessageSchema = v.object({
  type: v.literal("error"),
  message: v.string(),
});

const AnswerMessageSchema = v.object({
  type: v.literal("answer"),
  sdp: v.string(),
});

const IceCandidateMessageSchema = v.object({
  type: v.literal("ice_candidate"),
  candidate: v.string(),
  sdp_mid: v.nullable(v.string()),
  sdp_mline_index: v.nullable(v.number()),
});

export const MessageSchema = v.union([
  ErrorMessageSchema,
  AnswerMessageSchema,
  IceCandidateMessageSchema,
]);

export type WebRTCMessage = v.InferOutput<typeof MessageSchema>;

export const makeSignaling = (
  url: string,
  createWebSocket: (url: string) => WebSocket = (u) => new WebSocket(u),
) =>
  Effect.gen(function* () {
    const ws = yield* Effect.acquireRelease(
      Effect.async<WebSocket, WebSocketError>((resume) => {
        const s = createWebSocket(url);
        const onOpen = () => resume(Effect.succeed(s));
        const onError = (e: Event) =>
          resume(Effect.fail(new WebSocketError({ message: "Open failed", originalError: e })));
        s.addEventListener("open", onOpen);
        s.addEventListener("error", onError);
      }),
      (ws) =>
        Effect.sync(() => {
          ws.close();
        }),
    );

    const messages = Stream.async<WebRTCMessage, WebSocketError>((emit) => {
      const onMessage = (event: MessageEvent) => {
        if (typeof event.data !== "string") return;
        try {
          const json = JSON.parse(event.data);
          const result = v.safeParse(MessageSchema, json);
          if (result.success) {
            void emit.single(result.output);
          } else {
            // Silently ignore or maybe we should log? For now let's just ignore invalid messages to be safe
            // Or better, let's try to emit what we can
          }
        } catch {
          // ignore json parse error
        }
      };

      // We also need to handle errors and close during the stream lifetime
      const onError = (e: Event) => {
        void emit.fail(new WebSocketError({ message: "Runtime Error", originalError: e }));
      };
      const onClose = () => {
        void emit.fail(new WebSocketError({ message: "Closed" }));
      };

      ws.addEventListener("message", onMessage);
      ws.addEventListener("error", onError);
      ws.addEventListener("close", onClose);

      return Effect.sync(() => {
        ws.removeEventListener("message", onMessage);
        ws.removeEventListener("error", onError);
        ws.removeEventListener("close", onClose);
      });
    });

    const send = (msg: unknown) =>
      Effect.sync(() => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify(msg));
        }
      });

    return { ws, messages, send };
  });
