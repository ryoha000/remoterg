/**
 * WebSocket処理の純粋関数
 */

import type { Role, SessionState } from "./types";
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
  updateState: (newState: SessionState) => void
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
    const newState = removeConnection(currentState, role);
    updateState(newState);
  });

  server.addEventListener("error", (error) => {
    logWebSocketError(role, error);
  });
}

/**
 * WebSocketアップグレード処理
 */
export function handleWebSocketUpgrade(
  _request: Request,
  role: Role,
  sessionId: string,
  ctx: DurableObjectState,
  state: SessionState,
  getState: () => SessionState,
  updateState: (newState: SessionState) => void
): Response {
  console.log("handleWebSocketUpgrade", role, sessionId);
  const pair = new WebSocketPair();
  const [client, server] = Object.values(pair);

  // 接続を受け入れる
  ctx.acceptWebSocket(server);
  console.log("acceptWebSocket");

  // 接続を更新
  const newState = updateConnection(state, role, server);
  updateState(newState);

  // イベントハンドラを設定（最新の状態を取得する関数を渡す）
  setupWebSocketHandlers(server, role, getState, updateState);

  return new Response(null, {
    status: 101,
    webSocket: client,
  });
}
