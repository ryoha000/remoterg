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

export interface WebSocketWithRole extends WebSocket {
  __role?: Role;
}
