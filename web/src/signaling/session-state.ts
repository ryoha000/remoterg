/**
 * セッション状態管理の純粋関数
 */

import type { Role, SessionState, WebSocketWithRole } from "./types";

/**
 * 初期セッション状態を作成
 */
export function createSessionState(sessionId: string, ttl: number = 3600): SessionState {
  return {
    hostWs: null,
    viewerWs: null,
    hostRole: null,
    viewerRole: null,
    sessionId,
    ttl,
  };
}

/**
 * WebSocket接続を更新
 */
export function updateConnection(
  state: SessionState,
  role: Role,
  ws: WebSocket
): SessionState {
  const newState = { ...state };

  if (role === "host") {
    if (newState.hostWs) {
      newState.hostWs.close(1000, "New host connection");
    }
    newState.hostWs = ws;
    newState.hostRole = role;
  } else {
    if (newState.viewerWs) {
      newState.viewerWs.close(1000, "New viewer connection");
    }
    newState.viewerWs = ws;
    newState.viewerRole = role;
  }

  // WebSocketにrole情報を保存
  (ws as WebSocketWithRole).__role = role;

  return newState;
}

/**
 * WebSocket接続を削除
 */
export function removeConnection(
  state: SessionState,
  role: Role
): SessionState {
  const newState = { ...state };

  if (role === "host") {
    newState.hostWs = null;
    newState.hostRole = null;
  } else {
    newState.viewerWs = null;
    newState.viewerRole = null;
  }

  return newState;
}

/**
 * WebSocketからroleを取得
 */
export function getRoleFromWebSocket(
  state: SessionState,
  ws: WebSocket
): Role | null {
  if (ws === state.hostWs) {
    return "host";
  }
  if (ws === state.viewerWs) {
    return "viewer";
  }
  // フォールバック: __roleプロパティを確認
  return (ws as WebSocketWithRole).__role || null;
}

/**
 * 転送先のWebSocketを取得
 */
export function getTargetWebSocket(
  state: SessionState,
  fromRole: Role
): WebSocket | null {
  return fromRole === "host" ? state.viewerWs : state.hostWs;
}

/**
 * 転送先のroleを取得
 */
export function getTargetRole(fromRole: Role): Role {
  return fromRole === "host" ? "viewer" : "host";
}

