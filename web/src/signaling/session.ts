import { DurableObject } from "cloudflare:workers";
import * as v from "valibot";
import type { SessionState } from "./types";
import { WsAttachmentV1Schema } from "./types";
import { createSessionState, getRoleFromWebSocket, removeConnection } from "./session-state";
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
    const sessionId = ctx.id.toString();
    this.state = createSessionState(sessionId, 3600);

    // WebSocket Hibernation 対応: 接続中の WebSocket を復元
    this.restoreConnections(sessionId);
  }

  /**
   * WebSocket Hibernation 復帰時に接続を復元
   * constructor が再実行されるため、getWebSockets() で接続を列挙し、
   * deserializeAttachment() から role を取得して state を再構築する
   */
  private restoreConnections(sessionId: string): void {
    const websockets = this.ctx.getWebSockets();
    const totalConnections = websockets.length;

    console.log(
      `[SignalingSession] Restoring connections after hibernation: total=${totalConnections}, session_id=${sessionId}`,
    );

    let hostWs: WebSocket | null = null;
    let viewerWs: WebSocket | null = null;
    let invalidAttachmentCount = 0;
    let duplicateHostCount = 0;
    let duplicateViewerCount = 0;

    for (const ws of websockets) {
      try {
        const rawAttachment = ws.deserializeAttachment();
        if (!rawAttachment) {
          console.warn(
            `[SignalingSession] Invalid attachment detected (null/undefined), closing WebSocket.`,
          );
          ws.close(1000, "Invalid attachment");
          invalidAttachmentCount++;
          continue;
        }

        const attachment = v.parse(WsAttachmentV1Schema, rawAttachment);

        // session_id が一致しない場合は close（誤転送防止）
        if (attachment.session_id !== sessionId) {
          console.warn(
            `[SignalingSession] Session ID mismatch, closing WebSocket. expected=${sessionId}, got=${attachment.session_id}`,
          );
          ws.close(1000, "Session ID mismatch");
          invalidAttachmentCount++;
          continue;
        }

        // 同一 role が複数存在した場合は最新1本のみを残し、残りは close
        if (attachment.role === "host") {
          if (hostWs) {
            console.warn(
              `[SignalingSession] Duplicate host connection detected, closing older connection`,
            );
            hostWs.close(1000, "Duplicate role connection");
            duplicateHostCount++;
          }
          hostWs = ws;
          this.state.hostRole = "host";
        } else {
          if (viewerWs) {
            console.warn(
              `[SignalingSession] Duplicate viewer connection detected, closing older connection`,
            );
            viewerWs.close(1000, "Duplicate role connection");
            duplicateViewerCount++;
          }
          viewerWs = ws;
          this.state.viewerRole = "viewer";
        }

        console.log(
          `[SignalingSession] Restored ${attachment.role} connection, session_id=${attachment.session_id}`,
        );
      } catch (error) {
        console.error(`[SignalingSession] Error during connection restoration:`, error);
        ws.close(1000, "Restoration error");
        invalidAttachmentCount++;
      }
    }

    // state を更新
    this.state.hostWs = hostWs;
    this.state.viewerWs = viewerWs;

    // 復元結果をログ出力
    console.log(
      `[SignalingSession] Connection restoration completed: ` +
        `host=${hostWs ? "restored" : "none"}, ` +
        `viewer=${viewerWs ? "restored" : "none"}, ` +
        `invalid_attachment=${invalidAttachmentCount}, ` +
        `duplicate_host=${duplicateHostCount}, ` +
        `duplicate_viewer=${duplicateViewerCount}`,
    );
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const role = url.searchParams.get("role");
    // 注意: query param の session_id は Durable Object へのルーティング用途に限定
    // DO 内部では this.state.sessionId（= ctx.id.toString()）を唯一の正として使用する

    if (!validateRole(role)) {
      return new Response('Invalid role parameter. Must be "host" or "viewer"', { status: 400 });
    }

    // WebSocket upgrade
    if (request.headers.get("Upgrade") === "websocket") {
      // session_id は必ず state.sessionId（= DO id）を使用
      // attachment に保存される session_id は DO id と一致する必要がある
      return handleWebSocketUpgrade(
        request,
        role,
        this.state.sessionId,
        this.ctx,
        this.state,
        (newState) => {
          this.state = newState;
        },
      );
    }

    return new Response("Expected WebSocket upgrade", { status: 426 });
  }

  // Cloudflare Durable ObjectのwebSocketMessageメソッドを実装
  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void | Promise<void> {
    const role = getRoleFromWebSocket(this.state, ws);

    if (!role) {
      // attachment から role を取得できなかった場合（attachment 欠損/不正）
      logRoleNotFound(ws === this.state.hostWs, ws === this.state.viewerWs);
      // プロトコル破綻防止のため、サーバ側から close
      ws.close(1000, "Role not found: invalid or missing attachment");
      return;
    }

    const messageLength = typeof message === "string" ? message.length : message.byteLength;
    logWebSocketMessageMethod(role, typeof message, messageLength);

    const data = typeof message === "string" ? message : new TextDecoder().decode(message);

    handleMessage(this.state, role, data);
  }

  // Cloudflare Durable ObjectのwebSocketCloseメソッドを実装
  webSocketClose(
    ws: WebSocket,
    code: number,
    reason: string,
    wasClean: boolean,
  ): void | Promise<void> {
    const role = getRoleFromWebSocket(this.state, ws);
    logWebSocketCloseMethod(role, code, reason, wasClean);

    if (role) {
      this.state = removeConnection(this.state, role, ws);
    }
  }

  // Cloudflare Durable ObjectのwebSocketErrorメソッドを実装
  webSocketError(ws: WebSocket, error: unknown): void | Promise<void> {
    const role = getRoleFromWebSocket(this.state, ws);
    logWebSocketErrorMethod(role, error);
  }
}
