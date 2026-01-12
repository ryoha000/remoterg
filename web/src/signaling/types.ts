/**
 * シグナリング関連の型定義
 */

export type Role = "host" | "viewer";

export interface SessionState {
  hostWs: WebSocket | null;
  viewerWs: WebSocket | null;
  hostRole: Role | null;
  viewerRole: Role | null;
  sessionId: string;
  ttl: number;
}

export interface SignalingMessage {
  type: string;
  sdp?: string;
  codec?: string;
  candidate?: string;
  negotiation_id?: string;
  session_id?: string;
  [key: string]: unknown;
}

export interface EnrichedMessage extends SignalingMessage {
  session_id: string;
  negotiation_id: string;
}

/**
 * WebSocket attachment スキーマ（v1）
 * WebSocket Hibernation 対応のため、role と session_id を永続化
 */
export type WsAttachmentV1 = {
  v: 1;
  role: "host" | "viewer";
  session_id: string;
};
