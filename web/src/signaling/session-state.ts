/**
 * セッション状態管理の純粋関数
 */

import type { Role, SessionState, WsAttachmentV1 } from "./types";

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

  // 注意: role 情報は serializeAttachment() で永続化されるため、
  // ここでは __role プロパティを設定しない（WebSocket Hibernation 対応）

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
 * WebSocket Hibernation 対応: attachment から role を取得する（参照一致に依存しない）
 */
export function getRoleFromWebSocket(
  _state: SessionState,
  ws: WebSocket
): Role | null {
  try {
    const attachment = ws.deserializeAttachment() as WsAttachmentV1 | null;
    
    if (!attachment) {
      return null;
    }
    
    // v1 スキーマの検証
    if (attachment.v !== 1) {
      return null;
    }
    
    // role の検証
    if (attachment.role !== "host" && attachment.role !== "viewer") {
      return null;
    }
    
    return attachment.role;
  } catch (error) {
    console.error("[SignalingSession] Failed to deserialize attachment:", error);
    return null;
  }
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

