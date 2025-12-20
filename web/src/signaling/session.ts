import { DurableObject } from "cloudflare:workers";

/**
 * SignalingSession Durable Object
 *
 * WebRTCシグナリング用のセッション管理を行うDurable Object
 * hostとviewerの2つのWebSocket接続を管理し、メッセージを双方向にルーティングする
 */

export class SignalingSession extends DurableObject {
  private hostWs: WebSocket | null = null;
  private viewerWs: WebSocket | null = null;
  private hostRole: string | null = null;
  private viewerRole: string | null = null;
  private sessionId: string;
  private ttl: number = 3600; // 1時間のTTL

  constructor(ctx: DurableObjectState, env: Cloudflare.Env) {
    super(ctx, env);
    this.sessionId = ctx.id.toString();
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const role = url.searchParams.get("role");
    const sessionId = url.searchParams.get("session_id") || "fixed";

    if (!role || (role !== "host" && role !== "viewer")) {
      return new Response(
        'Invalid role parameter. Must be "host" or "viewer"',
        { status: 400 }
      );
    }

    // WebSocket upgrade
    if (request.headers.get("Upgrade") === "websocket") {
      return this.handleWebSocketUpgrade(request, role, sessionId);
    }

    return new Response("Expected WebSocket upgrade", { status: 426 });
  }

  private async handleWebSocketUpgrade(
    request: Request,
    role: string,
    sessionId: string
  ): Promise<Response> {
    console.log("handleWebSocketUpgrade", role, sessionId);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);

    // 接続を受け入れる
    this.ctx.acceptWebSocket(server);
    console.log("acceptWebSocket");

    // 既存の接続をチェック
    if (role === "host") {
      if (this.hostWs) {
        this.hostWs.close(1000, "New host connection");
      }
      this.hostWs = server;
      this.hostRole = role;
    } else {
      if (this.viewerWs) {
        this.viewerWs.close(1000, "New viewer connection");
      }
      this.viewerWs = server;
      this.viewerRole = role;
    }

    // WebSocketにrole情報を保存（webSocketMessageで使用するため）
    (server as any).__role = role;

    // メッセージハンドラを設定（addEventListenerとwebSocketMessageの両方を使用）
    console.log(`[SignalingSession] Setting up message handler for ${role}`);
    server.addEventListener("message", (event) => {
      console.log(
        `[SignalingSession] Message event received from ${role}, data type: ${typeof event.data}, length: ${
          event.data?.length || 0
        }`
      );
      this.handleMessage(role, event.data as string);
    });

    server.addEventListener("close", () => {
      console.log(`[SignalingSession] WebSocket closed for ${role}`);
      if (role === "host") {
        this.hostWs = null;
        this.hostRole = null;
      } else {
        this.viewerWs = null;
        this.viewerRole = null;
      }
    });

    server.addEventListener("error", (error) => {
      console.error(`WebSocket error for ${role}:`, error);
    });

    return new Response(null, {
      status: 101,
      webSocket: client,
    });
  }

  // Cloudflare Durable ObjectのwebSocketMessageメソッドを実装
  webSocketMessage(
    ws: WebSocket,
    message: string | ArrayBuffer
  ): void | Promise<void> {
    // WebSocketを比較してroleを判定
    let role: string | null = null;
    if (ws === this.hostWs) {
      role = "host";
    } else if (ws === this.viewerWs) {
      role = "viewer";
    } else {
      // フォールバック: __roleプロパティを確認
      role = ((ws as any).__role as string | undefined) || null;
    }

    if (!role) {
      console.error(
        "[SignalingSession] webSocketMessage: role not found for WebSocket"
      );
      console.error(
        `[SignalingSession] hostWs: ${this.hostWs === ws}, viewerWs: ${
          this.viewerWs === ws
        }`
      );
      return;
    }

    console.log(
      `[SignalingSession] webSocketMessage called from ${role}, message type: ${typeof message}, length: ${
        typeof message === "string" ? message.length : message.byteLength
      }`
    );

    if (typeof message === "string") {
      this.handleMessage(role, message);
    } else {
      // ArrayBufferの場合は文字列に変換
      const decoder = new TextDecoder();
      const text = decoder.decode(message);
      this.handleMessage(role, text);
    }
  }

  // Cloudflare Durable ObjectのwebSocketCloseメソッドを実装
  webSocketClose(
    ws: WebSocket,
    code: number,
    reason: string,
    wasClean: boolean
  ): void | Promise<void> {
    let role: string | null = null;
    if (ws === this.hostWs) {
      role = "host";
    } else if (ws === this.viewerWs) {
      role = "viewer";
    }

    console.log(
      `[SignalingSession] webSocketClose called for ${
        role || "unknown"
      }, code: ${code}, reason: ${reason}, wasClean: ${wasClean}`
    );

    if (ws === this.hostWs) {
      this.hostWs = null;
      this.hostRole = null;
    } else if (ws === this.viewerWs) {
      this.viewerWs = null;
      this.viewerRole = null;
    }
  }

  // Cloudflare Durable ObjectのwebSocketErrorメソッドを実装
  webSocketError(ws: WebSocket, error: unknown): void | Promise<void> {
    let role: string | null = null;
    if (ws === this.hostWs) {
      role = "host";
    } else if (ws === this.viewerWs) {
      role = "viewer";
    }
    console.error(
      `[SignalingSession] webSocketError for ${role || "unknown"}:`,
      error
    );
  }

  private handleMessage(fromRole: string, data: string): void {
    console.log(
      `[SignalingSession] handleMessage called from ${fromRole}, data length: ${
        data?.length || 0
      }`
    );
    try {
      const message = JSON.parse(data);
      const messageType = message.type || "unknown";
      const targetRole = fromRole === "host" ? "viewer" : "host";

      // 受信メッセージの詳細情報をログ出力
      let messageDetails = `type=${messageType}`;
      if (message.sdp) {
        const sdpPreview = message.sdp.substring(0, 100).replace(/\n/g, "\\n");
        messageDetails += `, sdp_length=${message.sdp.length}, sdp_preview=${sdpPreview}...`;
      }
      if (message.codec) {
        messageDetails += `, codec=${message.codec}`;
      }
      if (message.candidate) {
        const candidatePreview = message.candidate.substring(0, 50);
        messageDetails += `, candidate=${candidatePreview}...`;
      }

      console.log(
        `[SignalingSession] Received message from ${fromRole}: ${messageDetails}, session_id=${this.sessionId}`
      );

      // メッセージにsession_idとnegotiation_idを追加（必要に応じて）
      const enrichedMessage = {
        ...message,
        session_id: this.sessionId,
        negotiation_id: message.negotiation_id || "default",
      };

      // 相手側に転送
      const targetWs = fromRole === "host" ? this.viewerWs : this.hostWs;
      if (targetWs) {
        const targetState = targetWs.readyState;
        const targetStateText =
          targetState === WebSocket.CONNECTING
            ? "CONNECTING"
            : targetState === WebSocket.OPEN
            ? "OPEN"
            : targetState === WebSocket.CLOSING
            ? "CLOSING"
            : targetState === WebSocket.CLOSED
            ? "CLOSED"
            : "UNKNOWN";

        console.log(
          `[SignalingSession] Target WebSocket (${targetRole}) state: ${targetStateText} (${targetState}), OPEN=${WebSocket.OPEN}`
        );

        if (targetState === WebSocket.OPEN) {
          try {
            const messageJson = JSON.stringify(enrichedMessage);
            targetWs.send(messageJson);
            console.log(
              `[SignalingSession] Successfully forwarded ${messageType} from ${fromRole} to ${targetRole} (message_size=${messageJson.length} bytes)`
            );
          } catch (sendError) {
            console.error(
              `[SignalingSession] Failed to send message to ${targetRole}:`,
              sendError
            );
            console.error(
              `[SignalingSession] Error details - messageType: ${messageType}, targetRole: ${targetRole}, error: ${sendError}`
            );
          }
        } else {
          console.warn(
            `[SignalingSession] Target WebSocket (${targetRole}) is not OPEN (state: ${targetStateText}/${targetState}). Message type: ${messageType}. Message will be dropped.`
          );
          console.warn(
            `[SignalingSession] Current connections - hostWs: ${
              this.hostWs ? "connected" : "null"
            }, viewerWs: ${this.viewerWs ? "connected" : "null"}`
          );
        }
      } else {
        console.warn(
          `[SignalingSession] Target WebSocket (${targetRole}) is null. Message type: ${messageType}. Message will be dropped.`
        );
        console.warn(
          `[SignalingSession] Current connections - hostWs: ${
            this.hostWs ? "connected" : "null"
          }, viewerWs: ${this.viewerWs ? "connected" : "null"}`
        );
      }
    } catch (error) {
      console.error(
        "[SignalingSession] Failed to parse or forward message:",
        error
      );
      console.error(
        `[SignalingSession] Error details - fromRole: ${fromRole}, data_length: ${
          data?.length || 0
        }, error: ${error}`
      );
      if (error instanceof Error) {
        console.error(`[SignalingSession] Error stack: ${error.stack}`);
      }
    }
  }
}
