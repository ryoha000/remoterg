import { useRef, useState, useCallback, useEffect } from "react";
import { Effect, Queue, Fiber, Stream, Schedule } from "effect";
import {
  makeSignaling,
  type WebRTCMessage,
  WebSocketError,
  makeConnection,
  PeerConnectionError,
  setH264Preferences,
  createDataChannel,
  runDataChannel,
  type LlmConfig,
  runStatsLoop,
  type WebRTCStats,
  makeMediaStreamHandler,
  createMockWebSocket,
  createMockPeerConnection,
} from "@remoterg/webrtc";

export interface WebRTCOptions {
  signalUrl: string;
  sessionId: string;
  codec?: "h264" | "any";
  useMock?: boolean;
  onTrack?: (stream: MediaStream) => void;
  onConnectionStateChange?: (state: string) => void;
  onIceConnectionStateChange?: (state: string) => void;
  onScreenshot?: (blob: Blob, meta: { id: string; format: string; size: number }) => void;

  onAnalyzeResult?: (id: string, text: string) => void;
  onAnalyzeResultDelta?: (id: string, delta: string) => void;
  onAnalyzeDone?: (id: string) => void;
  onLlmConfig?: (config: LlmConfig) => void;
}

export type { WebRTCStats };

export function useWebRTC(options: WebRTCOptions) {
  const {
    signalUrl,
    sessionId,
    codec = "h264",
    useMock = false,
    onTrack,
    onConnectionStateChange,
    onIceConnectionStateChange,
    onScreenshot,

    onAnalyzeResult,
    onAnalyzeResultDelta,
    onAnalyzeDone,
    onLlmConfig,
  } = options;

  const [connectionState, setConnectionState] = useState<string>("disconnected");
  const [iceConnectionState, setIceConnectionState] = useState<string>("new");
  const [stats, setStats] = useState<WebRTCStats>({});
  const [logs, setLogs] = useState<Array<{ time: string; message: string; type: string }>>([]);

  // Queue to send signals to the running Effect
  const sendKeyQueue = useRef<Queue.Queue<{ key: string; down: boolean }> | null>(null);
  const screenshotRequestQueue = useRef<Queue.Queue<void> | null>(null);
  const analyzeRequestQueue = useRef<Queue.Queue<{ id: string; max_edge: number }> | null>(null);
  const mouseClickQueue = useRef<Queue.Queue<{ x: number; y: number; button: string }> | null>(null);

  const debugActionQueue = useRef<Queue.Queue<"close_ws" | "close_pc"> | null>(null);
  const getLlmConfigQueue = useRef<Queue.Queue<void> | null>(null);
  const updateLlmConfigQueue = useRef<Queue.Queue<LlmConfig> | null>(null);

  // To trigger manual disconnect/reconnect (using a counter to restart the effect)
  const [connectTrigger, setConnectTrigger] = useState(0);

  const addLog = useCallback((message: string, type: string = "info") => {
    const time = new Date().toLocaleTimeString();
    setLogs((prev) => [...prev, { time, message, type }].slice(-100));
  }, []);

  const sendKey = useCallback(
    (key: string, down: boolean = true) => {
      if (sendKeyQueue.current) {
        Effect.runFork(
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
      Effect.runFork(
        Queue.offer(screenshotRequestQueue.current, void 0).pipe(
          Effect.catchAll(() => Effect.void),
        ),
      );
      addLog("スクリーンショットリクエスト送信");
    } else {
      addLog("DataChannelが開いていません", "error");
    }
  }, [addLog]);

  const requestAnalyze = useCallback(
    (id: string, max_edge: number = 512) => {
      if (analyzeRequestQueue.current) {
        Effect.runFork(
          Queue.offer(analyzeRequestQueue.current, { id, max_edge }).pipe(
            Effect.catchAll(() => Effect.void),
          ),
        );
        addLog(`解析リクエスト送信: ${id} (max_edge: ${max_edge})`);
      } else {
        addLog("DataChannelが開いていません", "error");
      }
    },
    [addLog],
  );

  const sendMouseClick = useCallback(
    (x: number, y: number, button: string = "left") => {
      if (mouseClickQueue.current) {
        Effect.runFork(
          Queue.offer(mouseClickQueue.current, { x, y, button }).pipe(
            Effect.catchAll(() => Effect.void),
          ),
        );
      }
    },
    [],
  );

  const requestGetLlmConfig = useCallback(() => {
    if (getLlmConfigQueue.current) {
      Effect.runFork(
        Queue.offer(getLlmConfigQueue.current, void 0).pipe(Effect.catchAll(() => Effect.void)),
      );
      addLog("LLM設定取得リクエスト送信");
    } else {
      addLog("DataChannelが開いていません", "error");
    }
  }, [addLog]);

  const requestUpdateLlmConfig = useCallback(
    (config: LlmConfig) => {
      if (updateLlmConfigQueue.current) {
        Effect.runFork(
          Queue.offer(updateLlmConfigQueue.current, config).pipe(
            Effect.catchAll(() => Effect.void),
          ),
        );
        addLog("LLM設定更新リクエスト送信");
      } else {
        addLog("DataChannelが開いていません", "error");
      }
    },
    [addLog],
  );

  const simulateWsClose = useCallback(() => {
    if (debugActionQueue.current) {
      Effect.runFork(
        Queue.offer(debugActionQueue.current, "close_ws").pipe(Effect.catchAll(() => Effect.void)),
      );
      addLog("デバッグ: WebSocket切断をシミュレート");
    }
  }, [addLog]);

  const simulatePcClose = useCallback(() => {
    if (debugActionQueue.current) {
      Effect.runFork(
        Queue.offer(debugActionQueue.current, "close_pc").pipe(Effect.catchAll(() => Effect.void)),
      );
      addLog("デバッグ: PeerConnection切断をシミュレート");
    }
  }, [addLog]);

  const connect = useCallback(() => {
    setConnectTrigger((c) => c + 1);
  }, []);

  const manualDisconnect = useCallback(() => {
    setConnectTrigger(0);
  }, []);

  useEffect(() => {
    if (connectTrigger === 0) {
      setConnectionState("disconnected");
      setIceConnectionState("new");
      return;
    }

    const program = Effect.gen(function* () {
      // --- Real WebRTC Logic ---
      yield* Effect.sync(() => {
        setConnectionState("connecting");
        addLog("WebSocket接続を開始...");
      });

      const keyQ = yield* Queue.unbounded<{ key: string; down: boolean }>();
      const screenQ = yield* Queue.unbounded<void>();
      const analyzeQ = yield* Queue.unbounded<{ id: string; max_edge: number }>();
      const mouseClickQ = yield* Queue.unbounded<{ x: number; y: number; button: string }>();

      const debugQ = yield* Queue.unbounded<"close_ws" | "close_pc">();
      const getLlmConfigQ = yield* Queue.unbounded<void>();
      const updateLlmConfigQ = yield* Queue.unbounded<LlmConfig>();
      const signalingQueue = yield* Queue.unbounded<WebRTCMessage>();

      yield* Effect.acquireRelease(
        Effect.sync(() => {
          sendKeyQueue.current = keyQ;
          screenshotRequestQueue.current = screenQ;
          analyzeRequestQueue.current = analyzeQ;
          mouseClickQueue.current = mouseClickQ;

          debugActionQueue.current = debugQ;
          getLlmConfigQueue.current = getLlmConfigQ;
          updateLlmConfigQueue.current = updateLlmConfigQ;
        }),
          () =>
            Effect.sync(() => {
              if (sendKeyQueue.current === keyQ) sendKeyQueue.current = null;
              if (screenshotRequestQueue.current === screenQ) screenshotRequestQueue.current = null;
              if (analyzeRequestQueue.current === analyzeQ) analyzeRequestQueue.current = null;
              if (mouseClickQueue.current === mouseClickQ) mouseClickQueue.current = null;

              if (debugActionQueue.current === debugQ) debugActionQueue.current = null;
              if (getLlmConfigQueue.current === getLlmConfigQ) getLlmConfigQueue.current = null;
              if (updateLlmConfigQueue.current === updateLlmConfigQ) updateLlmConfigQueue.current = null;
            }),
      );

      // 1. Signaling
      const wsUrl = `${signalUrl}?session_id=${sessionId}&role=viewer`;

      // Error handling for Signaling
      const signalingEffect = makeSignaling(
        wsUrl,
        useMock ? createMockWebSocket : undefined,
      );

      const { ws, messages: wsMessages, send: sendWs } = yield* signalingEffect;
      yield* Effect.sync(() => addLog("WebSocket接続確立", "success"));

      // 2. PeerConnection
      const config: RTCConfiguration = {
        iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }],
      };
      const {
        pc,
        connectionState: pcStateStream,
        iceConnectionState: iceStateStream,
        iceCandidates: iceCandidateStream,
        track: trackStream,
      } = yield* makeConnection(
        config,
        useMock ? createMockPeerConnection : undefined,
      );

      yield* Effect.sync(() => addLog("PeerConnectionを作成..."));

      // 3. Setup Transceivers & Codecs
      yield* Effect.sync(() => {
        pc.addTransceiver("video", { direction: "recvonly" });
        pc.addTransceiver("audio", { direction: "recvonly" });
      });

      if (codec === "h264") {
        const applied = yield* setH264Preferences(pc);
        if (applied) addLog("H.264 codec preferenceを適用", "success");
        else addLog("H.264 codec preference適用失敗 (or no codec found)", "warning");
      }

      // 4. Data Channel (Creation)
      const dc = yield* createDataChannel(pc);

      // --- Flows ---

      // Send Offer
      const sendOffer = Effect.gen(function* () {
        const offer = yield* Effect.promise(() => pc.createOffer());
        yield* Effect.promise(() => pc.setLocalDescription(offer));
        addLog("Offerを作成・設定しました", "success");

        yield* sendWs({ type: "offer", sdp: offer.sdp, codec });
        addLog("Offerを送信しました", "success");
      });

      // Handle Incoming Messages (Signaling)
      // This stream needs to stay alive
      // Handle Incoming Messages (Signaling) - Producer
      const handleSignalingMessages = wsMessages.pipe(
        Stream.runForEach((msg: WebRTCMessage) =>
          Effect.gen(function* () {
            addLog(`受信: ${msg.type}`);
            if (msg.type === "error") {
              addLog(`サーバーエラー: ${msg.message}`, "error");
              yield* Effect.fail(new WebSocketError({ message: msg.message }));
            } else {
              yield* Queue.offer(signalingQueue, msg);
            }
          }),
        ),
        Effect.tapError((e) => Effect.sync(() => addLog(`Signaling Loop Error: ${e}`, "error"))),
      );

      // Handle WebRTC Signaling Operations - Consumer
      const handleWebRTCOperations = Queue.take(signalingQueue).pipe(
        Effect.flatMap((msg) =>
          Effect.gen(function* () {
            if (msg.type === "answer") {
              addLog("Answer受信");
              yield* Effect.promise(() =>
                pc.setRemoteDescription({ type: "answer", sdp: msg.sdp }),
              );
              addLog("Answer設定完了", "success");
            } else if (msg.type === "ice_candidate") {
              const candidate = new RTCIceCandidate({
                candidate: msg.candidate,
                sdpMid: msg.sdp_mid,
                sdpMLineIndex: msg.sdp_mline_index,
              });
              yield* Effect.promise(() => pc.addIceCandidate(candidate)).pipe(
                Effect.catchAll((e) =>
                  Effect.sync(() => addLog(`ICE Candidate Error: ${e}`, "error")),
                ),
              );
              addLog("ICE candidate追加", "success");
            }
          }).pipe(
            // Catch PC errors so the consumer doesn't die and kill the session
            Effect.catchAll((e) =>
              Effect.sync(() => addLog(`WebRTC Signaling Processing Error: ${e}`, "error")),
            ),
          ),
        ),
        Effect.forever,
      );

      // Handle Outgoing ICE Candidates
      const handleOutgoingIce = iceCandidateStream.pipe(
        Stream.runForEach((candidate) =>
          Effect.gen(function* () {
            yield* sendWs({
              type: "ice_candidate",
              candidate: candidate.candidate,
              sdp_mid: candidate.sdpMid,
              sdp_mline_index: candidate.sdpMLineIndex,
            });
            addLog("ICE candidate送信");
          }),
        ),
      );

      // Handle Connection State
      const handleConnectionState = pcStateStream.pipe(
        Stream.runForEach((state) =>
          Effect.sync(() => {
            addLog(`接続状態: ${state}`);
            setConnectionState(state);
            onConnectionStateChange?.(state);
          }),
        ),
      );

      const handleIceConnectionState = iceStateStream.pipe(
        Stream.runForEach((state) =>
          Effect.sync(() => {
            addLog(`ICE接続状態: ${state}`);
            setIceConnectionState(state);
            onIceConnectionStateChange?.(state);
          }),
        ),
      );

      // Handle Tracks
      const handleTracks = makeMediaStreamHandler(trackStream, (stream) => {
        addLog("ストリームを受信", "success");
        onTrack?.(stream);
      });

      // Handle Data Channel Loop (Non-blocking / parallel)
      const handleDataChannelLoop = runDataChannel(
        dc,
        keyQ,
        screenQ,
        analyzeQ,
        mouseClickQ,
        () => addLog("DataChannel OPEN", "success"),
        (blob, meta) => {
          addLog("スクリーンショット受信", "success");
          onScreenshot?.(blob, meta);
        },
        (id, text) => {
          addLog("解析結果受信", "success");
          onAnalyzeResult?.(id, text);
        },
        (id, delta) => {
          onAnalyzeResultDelta?.(id, delta);
        },
        (id) => {
            onAnalyzeDone?.(id);
        },
        getLlmConfigQ,
        updateLlmConfigQ,
        (config) => {
          addLog("LLM設定受信", "success");
          onLlmConfig?.(config);
        },
      ).pipe(
        // Retry logic for DataChannel
        Effect.retry(Schedule.fixed("1 second")),
        Effect.tapError((e) =>
          Effect.sync(() => addLog(`DataChannel Permanent Error: ${e}`, "error")),
        ),
      );

      // Stats Loop (Independent)
      const handleStats = runStatsLoop(pc, setStats).pipe(
        // Retry logic for Stats
        Effect.retry(Schedule.fixed("1 second")),
        Effect.catchAll((e) => Effect.sync(() => console.error("Stats Error", e))),
      );

      // Debug Loop
      const handleDebug = Queue.take(debugQ).pipe(
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

      // Execute Sending Offer
      yield* sendOffer;

      // Run all background processes
      yield* Effect.all(
        [
          handleSignalingMessages,
          handleWebRTCOperations,
          handleOutgoingIce,
          handleConnectionState,
          handleIceConnectionState,
          handleTracks,
          handleDataChannelLoop,
          handleStats,
          handleDebug,
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
    onScreenshot,
    onAnalyzeResult,
    onAnalyzeResultDelta,
    onAnalyzeDone,
    onLlmConfig,
    useMock,
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
    requestAnalyze,
    sendMouseClick,
    requestGetLlmConfig,
    requestUpdateLlmConfig,
    simulateWsClose,
    simulatePcClose,
  };
}
