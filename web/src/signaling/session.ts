import { DurableObject } from "cloudflare:workers";
import type { SessionState } from "./types";
import {
  createSessionState,
  getRoleFromWebSocket,
  removeConnection,
} from "./session-state";
import { handleMessage } from "./message-handler";
import { validateRole, handleWebSocketUpgrade } from "./websocket-handler";
import {
  logWebSocketMessageMethod,
  logWebSocketCloseMethod,
  logWebSocketErrorMethod,
  logRoleNotFound,
} from "./logger";

/**
 * SignalingSession Durable Object
 *
 * WebRTCシグナリング用のセッション管理を行うDurable Object
 * hostとviewerの2つのWebSocket接続を管理し、メッセージを双方向にルーティングする
 */
export class SignalingSession extends DurableObject {
  private state: SessionState;

  constructor(ctx: DurableObjectState, env: Cloudflare.Env) {
    super(ctx, env);
    this.state = createSessionState(ctx.id.toString(), 3600);
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const role = url.searchParams.get("role");
    const sessionId = url.searchParams.get("session_id") || "fixed";

    if (!validateRole(role)) {
      return new Response(
        'Invalid role parameter. Must be "host" or "viewer"',
        { status: 400 }
      );
    }

    // WebSocket upgrade
    if (request.headers.get("Upgrade") === "websocket") {
      return handleWebSocketUpgrade(
        request,
        role,
        sessionId,
        this.ctx,
        this.state,
        () => this.state,
        (newState) => {
          this.state = newState;
        }
      );
    }

    return new Response("Expected WebSocket upgrade", { status: 426 });
  }

  // Cloudflare Durable ObjectのwebSocketMessageメソッドを実装
  webSocketMessage(
    ws: WebSocket,
    message: string | ArrayBuffer
  ): void | Promise<void> {
    const role = getRoleFromWebSocket(this.state, ws);

    if (!role) {
      logRoleNotFound(ws === this.state.hostWs, ws === this.state.viewerWs);
      return;
    }

    const messageLength =
      typeof message === "string" ? message.length : message.byteLength;
    logWebSocketMessageMethod(role, typeof message, messageLength);

    const data =
      typeof message === "string" ? message : new TextDecoder().decode(message);

    handleMessage(this.state, role, data);
  }

  // Cloudflare Durable ObjectのwebSocketCloseメソッドを実装
  webSocketClose(
    ws: WebSocket,
    code: number,
    reason: string,
    wasClean: boolean
  ): void | Promise<void> {
    const role = getRoleFromWebSocket(this.state, ws);
    logWebSocketCloseMethod(role, code, reason, wasClean);

    if (role) {
      this.state = removeConnection(this.state, role);
    }
  }

  // Cloudflare Durable ObjectのwebSocketErrorメソッドを実装
  webSocketError(ws: WebSocket, error: unknown): void | Promise<void> {
    const role = getRoleFromWebSocket(this.state, ws);
    logWebSocketErrorMethod(role, error);
  }
}
