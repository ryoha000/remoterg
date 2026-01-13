/**
 * WebSocket処理の純粋関数
 */

import type { Role, SessionState, WsAttachmentV1 } from "./types";
import { updateConnection, removeConnection } from "./session-state";
import { handleMessage } from "./message-handler";
import {
  logWebSocketConnection,
  logWebSocketMessageEvent,
  logWebSocketClose,
  logWebSocketError,
} from "./logger";

/**
 * roleパラメータの検証
 */
export function validateRole(role: string | null): role is Role {
  return role === "host" || role === "viewer";
}

/**
 * WebSocketイベントハンドラを設定
 */
export function setupWebSocketHandlers(
  server: WebSocket,
  role: Role,
  getState: () => SessionState,
  updateState: (newState: SessionState) => void,
): void {
  logWebSocketConnection(role);

  server.addEventListener("message", (event) => {
    const data = event.data as string;
    logWebSocketMessageEvent(role, typeof event.data, event.data?.length || 0);
    const currentState = getState();
    handleMessage(currentState, role, data);
  });

  server.addEventListener("close", () => {
    logWebSocketClose(role);
    const currentState = getState();
    const newState = removeConnection(currentState, role, server);
    updateState(newState);
  });

  server.addEventListener("error", (error) => {
    logWebSocketError(role, error);
  });
}

/**
 * WebSocketアップグレード処理
 *
 * @param sessionId - Durable Object ID（ctx.id.toString()）を指定すること
 *                    この値は state.sessionId と一致し、attachment に保存される
 *                    WebSocket Hibernation 復帰時の接続復元で使用される
 */
export function handleWebSocketUpgrade(
  _request: Request,
  role: Role,
  sessionId: string,
  ctx: DurableObjectState,
  state: SessionState,
  updateState: (newState: SessionState) => void,
): Response {
  console.log("handleWebSocketUpgrade", role, sessionId);
  const pair = new WebSocketPair();
  const [client, server] = Object.values(pair);

  // 接続を更新（acceptWebSocket前に状態を更新する必要がある）
  const newState = updateConnection(state, role, server);
  updateState(newState);

  // 接続を受け入れる
  ctx.acceptWebSocket(server);
  console.log("acceptWebSocket", role);

  // WebSocket Hibernation 対応: attachment に role と session_id を永続化
  // session_id は必ず state.sessionId（= DO id）を使用し、復帰時の restoreConnections() で
  // attachment.session_id === state.sessionId の一致検証が行われる
  const attachment: WsAttachmentV1 = {
    v: 1,
    role,
    session_id: sessionId,
  };
  server.serializeAttachment(attachment);
  console.log(`[SignalingSession] Serialized attachment for ${role}, session_id=${sessionId}`);

  // 注意: setupWebSocketHandlers は不要
  // Cloudflare Durable Objectsでは、acceptWebSocket()後に
  // webSocketMessage/webSocketClose/webSocketErrorメソッドが自動的に呼ばれる

  return new Response(null, {
    status: 101,
    webSocket: client,
  });
}
