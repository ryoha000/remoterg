import { useRef, useState, useCallback, useEffect } from "react";
import { Effect, Console, Schedule, Stream, Queue, Duration, Fiber, Data } from "effect";
import { env } from "@/env";

export interface WebRTCOptions {
  signalUrl: string;
  sessionId: string;
  codec?: "h264" | "any";
  onTrack?: (stream: MediaStream) => void;
  onConnectionStateChange?: (state: string) => void;
  onIceConnectionStateChange?: (state: string) => void;
}

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

// Helper interface for video element with non-standard captureStream
interface VideoElementWithCapture extends HTMLVideoElement {
  captureStream?(): MediaStream;
  mozCaptureStream?(): MediaStream;
}

// Errors
class WebSocketError extends Data.TaggedError("WebSocketError")<{
  message: string;
  originalError?: unknown;
}> {}

class PeerConnectionError extends Data.TaggedError("PeerConnectionError")<{
  message: string;
  originalError?: unknown;
}> {}
import * as v from "valibot";

// Message Schemas
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

const MessageSchema = v.union([ErrorMessageSchema, AnswerMessageSchema, IceCandidateMessageSchema]);

export function useWebRTC(options: WebRTCOptions) {
  const {
    signalUrl,
    sessionId,
    codec = "h264",
    onTrack,
    onConnectionStateChange,
    onIceConnectionStateChange,
  } = options;

  const [connectionState, setConnectionState] = useState<string>("disconnected");
  const [iceConnectionState, setIceConnectionState] = useState<string>("new");
  const [stats, setStats] = useState<WebRTCStats>({});
  const [logs, setLogs] = useState<Array<{ time: string; message: string; type: string }>>([]);

  // Queue to send signals to the running Effect
  const sendKeyQueue = useRef<Queue.Queue<{ key: string; down: boolean }> | null>(null);
  const screenshotRequestQueue = useRef<Queue.Queue<void> | null>(null);
  const debugActionQueue = useRef<Queue.Queue<"close_ws" | "close_pc"> | null>(null);

  // To trigger manual disconnect/reconnect (using a counter to restart the effect)
  const [connectTrigger, setConnectTrigger] = useState(0);

  const addLog = useCallback((message: string, type: string = "info") => {
    const time = new Date().toLocaleTimeString();
    setLogs((prev) => [...prev, { time, message, type }]);
  }, []);

  const sendKey = useCallback(
    (key: string, down: boolean = true) => {
      if (sendKeyQueue.current) {
        Effect.runSync(
          Queue.offer(sendKeyQueue.current, { key, down }).pipe(Effect.catchAll(() => Effect.void)),
        );
        addLog(`キー送信: ${key} (down: ${down})`);
      } else {
        addLog("DataChannelが開いていません", "error");
      }
    },
    [addLog],
  );

  const requestScreenshot = useCallback(() => {
    if (screenshotRequestQueue.current) {
      Effect.runSync(
        Queue.offer(screenshotRequestQueue.current, void 0).pipe(
          Effect.catchAll(() => Effect.void),
        ),
      );
      addLog("スクリーンショットリクエスト送信");
    } else {
      addLog("DataChannelが開いていません", "error");
    }
  }, [addLog]);

  const simulateWsClose = useCallback(() => {
    if (debugActionQueue.current) {
      Effect.runSync(
        Queue.offer(debugActionQueue.current, "close_ws").pipe(Effect.catchAll(() => Effect.void)),
      );
      addLog("デバッグ: WebSocket切断をシミュレート");
    }
  }, [addLog]);

  const simulatePcClose = useCallback(() => {
    if (debugActionQueue.current) {
      Effect.runSync(
        Queue.offer(debugActionQueue.current, "close_pc").pipe(Effect.catchAll(() => Effect.void)),
      );
      addLog("デバッグ: PeerConnection切断をシミュレート");
    }
  }, [addLog]);

  const connect = useCallback(() => {
    setConnectTrigger((c) => c + 1);
  }, []);

  const manualDisconnect = useCallback(() => {
    // Simply stop the effect by resetting the trigger.
    // The cleanup function of the useEffect will handle state resets.
    setConnectTrigger(0);
  }, []);

  useEffect(() => {
    if (connectTrigger === 0) {
      // Ensure state is reset when not connected
      setConnectionState("disconnected");
      setIceConnectionState("new");
      return;
    }

    const program = Effect.gen(function* () {
      // --- Mock Mode Check ---
      if (env.VITE_USE_MOCK === "true") {
        yield* Console.info("Mock Mode: Starting...");
        addLog("Mock Mode: Starting sequence...", "info");

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
        addLog("Mock Mode: Video playing", "success");

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
          addLog("Mock Mode: Connected", "success");
        });

        yield* Effect.never;
      }

      // --- Real WebRTC Logic ---
      yield* Effect.sync(() => {
        setConnectionState("connecting");
        addLog("WebSocket接続を開始...");
      });

      const keyQ = yield* Queue.unbounded<{ key: string; down: boolean }>();
      const screenQ = yield* Queue.unbounded<void>();
      const debugQ = yield* Queue.unbounded<"close_ws" | "close_pc">();

      yield* Effect.sync(() => {
        sendKeyQueue.current = keyQ;
        screenshotRequestQueue.current = screenQ;
        debugActionQueue.current = debugQ;
      });

      // Acquire WebSocket
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const wsUrl = `${protocol}//${window.location.host}${signalUrl}?session_id=${sessionId}&role=viewer`;

      const ws = yield* Effect.acquireRelease(
        Effect.async<WebSocket, WebSocketError>((resume) => {
          const s = new WebSocket(wsUrl);
          const onOpen = () => resume(Effect.succeed(s));
          const onError = (e: Event) =>
            resume(Effect.fail(new WebSocketError({ message: "Open failed", originalError: e })));
          s.addEventListener("open", onOpen);
          s.addEventListener("error", onError);
        }),
        (ws) =>
          Effect.sync(() => {
            // cleanup listeners not strictly necessary if ws is closed, but good practice if we were keeping it
            ws.close();
            addLog("WebSocket接続が閉じられました");
          }),
      );

      yield* Effect.sync(() => addLog("WebSocket接続確立", "success"));

      // Acquire PeerConnection
      const pc = yield* Effect.acquireRelease(
        Effect.sync(() => {
          const config: RTCConfiguration = {
            iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }],
          };
          return new RTCPeerConnection(config);
        }),
        (pc) =>
          Effect.sync(() => {
            pc.close();
          }),
      );
      yield* Effect.sync(() => addLog("PeerConnectionを作成..."));

      // Setup Transceivers
      yield* Effect.sync(() => {
        pc.addTransceiver("video", { direction: "recvonly" });
        pc.addTransceiver("audio", { direction: "recvonly" });
        if (codec === "h264") {
          try {
            const capabilities = RTCRtpSender.getCapabilities("video");
            const codecs = (capabilities?.codecs ?? []).filter(
              (c) =>
                c.mimeType === "video/H264" &&
                (c.sdpFmtpLine ?? "").includes("packetization-mode=1"),
            );
            const transceiver = pc.getTransceivers().find((t) => t.receiver.track.kind === "video");
            if (
              codecs.length > 0 &&
              transceiver &&
              typeof transceiver.setCodecPreferences === "function"
            ) {
              transceiver.setCodecPreferences(codecs);
              addLog(`H.264 codec preferenceを適用 (${codecs.length}件)`, "success");
            }
          } catch (e) {
            addLog(`codec preference設定エラー: ${String(e)}`, "error");
          }
        }
      });

      // Create DataChannel
      const dc = yield* Effect.acquireRelease(
        Effect.sync(() => pc.createDataChannel("input", { ordered: true })),
        (dc) => Effect.sync(() => dc.close()),
      );

      // --- Event Streams ---

      const wsMessages = Stream.async<string, WebSocketError>((emit) => {
        const onMessage = (event: MessageEvent) => {
          void emit.single(event.data);
        };
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

      const iceCandidates = Stream.async<RTCIceCandidate>((emit) => {
        pc.onicecandidate = (event) => {
          if (event.candidate) void emit.single(event.candidate);
        };
      });

      // Connection State Stream
      const connectionStateStream = Stream.async<void>((emit) => {
        const handler = () => {
          void emit.single(void 0);
        };
        pc.addEventListener("connectionstatechange", handler);
        return Effect.sync(() => {
          pc.removeEventListener("connectionstatechange", handler);
        });
      });

      // ICE Connection State Stream
      const iceConnectionStateStream = Stream.async<void>((emit) => {
        const handler = () => {
          void emit.single(void 0);
        };
        pc.addEventListener("iceconnectionstatechange", handler);
        return Effect.sync(() => {
          pc.removeEventListener("iceconnectionstatechange", handler);
        });
      });

      // Track Stream
      const trackStream = Stream.async<RTCTrackEvent>((emit) => {
        const handler = (event: RTCTrackEvent) => {
          void emit.single(event);
        };
        pc.addEventListener("track", handler);
        return Effect.sync(() => {
          pc.removeEventListener("track", handler);
        });
      });

      // --- Flows ---

      // Send Offer
      const sendOffer = Effect.gen(function* () {
        const offer = yield* Effect.promise(() => pc.createOffer());
        yield* Effect.promise(() => pc.setLocalDescription(offer));
        addLog("Offerを作成・設定しました", "success");

        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: "offer", sdp: offer.sdp, codec }));
          addLog("Offerを送信しました", "success");
        }
      });

      yield* sendOffer;

      // Process Incoming Messages
      const handleWsParams = wsMessages.pipe(
        Stream.runForEach((data) =>
          Effect.gen(function* () {
            if (typeof data !== "string") return;
            let msg: unknown;
            try {
              msg = JSON.parse(data);
            } catch {
              return;
            }

            if (typeof msg !== "object" || msg === null) return;

            const parseResult = v.safeParse(MessageSchema, msg);
            if (!parseResult.success) {
              addLog(`受信メッセージのパースエラー: ${parseResult.issues[0].message}`, "error");
              return;
            }
            const safeMsg = parseResult.output;

            addLog(`受信: ${safeMsg.type}`);

            if (safeMsg.type === "error") {
              addLog(`サーバーエラー: ${safeMsg.message}`, "error");
              yield* Effect.fail(new WebSocketError({ message: "Server reported error" }));
            } else if (safeMsg.type === "answer") {
              addLog("Answer受信");
              yield* Effect.promise(() =>
                pc.setRemoteDescription({ type: "answer", sdp: safeMsg.sdp }),
              );
              addLog("Answer設定完了", "success");
            } else if (safeMsg.type === "ice_candidate") {
              const candidate = new RTCIceCandidate({
                candidate: safeMsg.candidate,
                sdpMid: safeMsg.sdp_mid,
                sdpMLineIndex: safeMsg.sdp_mline_index,
              });
              try {
                yield* Effect.promise(() => pc.addIceCandidate(candidate));
                addLog("ICE candidate追加", "success");
              } catch (e) {
                addLog(`ICE candidate追加エラー: ${String(e)}`, "error");
              }
            }
          }),
        ),
      );

      // Send ICE Candidates
      const handleOutgoingIceCandidates = iceCandidates.pipe(
        Stream.runForEach((candidate) =>
          Effect.sync(() => {
            if (ws.readyState === WebSocket.OPEN) {
              ws.send(
                JSON.stringify({
                  type: "ice_candidate",
                  candidate: candidate.candidate,
                  sdp_mid: candidate.sdpMid,
                  sdp_mline_index: candidate.sdpMLineIndex,
                }),
              );
              addLog("ICE candidate送信");
            }
          }),
        ),
      );

      // Data Channel Logic
      const handleDataChannel = Effect.gen(function* () {
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

        // Race between waiting for open and waiting for close (if closed before open)
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

        yield* Effect.race(waitForOpen, waitForCloseInitial);

        if (dc.readyState !== "open") {
          addLog("DataChannel failed to open (closed before open)", "error");
          return;
        }

        addLog("DataChannel OPEN", "success");

        const loops = Effect.gen(function* () {
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

          yield* Effect.all([keepAlive, processKeys, processScreens], {
            concurrency: "unbounded",
          });
        });

        const waitForClose = Effect.async<void>((resume) => {
          const onClose = () => {
            dc.removeEventListener("close", onClose);
            dc.removeEventListener("error", onError);
            addLog("DataChannel Closed");
            resume(Effect.void);
          };
          const onError = (e: Event) => {
            dc.removeEventListener("close", onClose);
            dc.removeEventListener("error", onError);
            addLog(`DataChannel Error: ${"type" in e ? e.type : "Unknown Error"}`);
            resume(Effect.void);
          };
          dc.addEventListener("close", onClose);
          dc.addEventListener("error", onError);
        });

        // Run loops until closed or error
        yield* Effect.race(loops, waitForClose);
      });

      // Stats Loop
      const statsLoop = Effect.repeat(
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

          setStats({ inbound, track });
        }),
        Schedule.spaced(Duration.seconds(2)),
      );

      // Debug Action Loop
      const debugLoop = Queue.take(debugQ).pipe(
        Effect.flatMap((action) => {
          if (action === "close_ws") {
            return Effect.sync(() => {
              addLog("DEBUG: Closing WebSocket...");
              ws.close();
            });
          } else if (action === "close_pc") {
            return Effect.sync(() => {
              addLog("DEBUG: Closing PeerConnection...");
              pc.close();
            }).pipe(
              Effect.flatMap(() =>
                Effect.fail(new PeerConnectionError({ message: "Simulated PC Close" })),
              ),
            );
          }
          return Effect.void;
        }),
        Effect.forever,
      );

      // Process Connection State Changes
      const handleConnectionState = connectionStateStream.pipe(
        Stream.runForEach(() =>
          Effect.sync(() => {
            const state = pc.connectionState;
            addLog(`接続状態: ${state}`);
            setConnectionState(state);
            onConnectionStateChange?.(state);
          }),
        ),
      );

      // Process ICE Connection State Changes
      const handleIceConnectionState = iceConnectionStateStream.pipe(
        Stream.runForEach(() =>
          Effect.sync(() => {
            const state = pc.iceConnectionState;
            addLog(`ICE接続状態: ${state}`);
            setIceConnectionState(state);
            onIceConnectionStateChange?.(state);
          }),
        ),
      );

      // Process Incoming Tracks
      let remoteStream: MediaStream | null = null;
      const handleTracks = trackStream.pipe(
        Stream.runForEach((event) =>
          Effect.sync(() => {
            addLog(
              `ストリームを受信 (tracks=${event.streams?.[0]?.getTracks().length ?? 0})`,
              "success",
            );
            if (event.track) {
              addLog(`トラック情報 kind=${event.track.kind} id=${event.track.id}`, "success");
              if (!remoteStream) {
                remoteStream = new MediaStream();
              }
              remoteStream.addTrack(event.track);
              onTrack?.(remoteStream);
            }
          }),
        ),
      );

      yield* Effect.all(
        [
          handleWsParams,
          handleOutgoingIceCandidates,
          handleDataChannel,
          statsLoop,
          debugLoop,
          handleConnectionState,
          handleIceConnectionState,
          handleTracks,
        ],
        { concurrency: "unbounded", discard: true },
      );
    }).pipe(
      Effect.scoped,
      Effect.catchAll((e) =>
        Effect.sync(() => {
          addLog(`Error in WebRTC Effect: ${e instanceof Error ? e.message : String(e)}`, "error");
          setConnectionState("error");
        }),
      ),
    );

    const runner = Effect.runFork(program);

    return () => {
      Effect.runFork(Fiber.interrupt(runner));
      addLog("Hook cleanup / disconnected");
      setConnectionState("disconnected");
      setIceConnectionState("new");
    };
  }, [
    connectTrigger,
    signalUrl,
    sessionId,
    codec,
    addLog,
    onConnectionStateChange,
    onIceConnectionStateChange,
    onTrack,
  ]);

  return {
    connectionState,
    iceConnectionState,
    stats,
    logs,
    connect,
    disconnect: manualDisconnect,
    sendKey,
    requestScreenshot,
    simulateWsClose,
    simulatePcClose,
  };
}
