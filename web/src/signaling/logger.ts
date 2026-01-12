/**
 * ログ処理の純粋関数
 */

import type { Role, SignalingMessage } from "./types";

/**
 * WebSocket状態を文字列に変換
 */
function getWebSocketStateText(state: number): string {
  if (state === WebSocket.CONNECTING) return "CONNECTING";
  if (state === WebSocket.OPEN) return "OPEN";
  if (state === WebSocket.CLOSING) return "CLOSING";
  if (state === WebSocket.CLOSED) return "CLOSED";
  return "UNKNOWN";
}

/**
 * メッセージ受信ログを出力
 */
export function logMessageReceived(
  fromRole: Role,
  message: SignalingMessage,
  sessionId: string
): void {
  const messageType = message.type || "unknown";
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
    `[SignalingSession] Received message from ${fromRole}: ${messageDetails}, session_id=${sessionId}`
  );
}

/**
 * WebSocket状態ログを出力
 */
export function logWebSocketState(
  targetRole: Role,
  state: number,
  _messageType: string
): void {
  const stateText = getWebSocketStateText(state);
  console.log(
    `[SignalingSession] Target WebSocket (${targetRole}) state: ${stateText} (${state}), OPEN=${WebSocket.OPEN}`
  );
}

/**
 * メッセージ転送成功ログを出力
 */
export function logMessageForwarded(
  fromRole: Role,
  toRole: Role,
  messageType: string,
  messageSize: number
): void {
  console.log(
    `[SignalingSession] Successfully forwarded ${messageType} from ${fromRole} to ${toRole} (message_size=${messageSize} bytes)`
  );
}

/**
 * メッセージ転送失敗ログを出力
 */
export function logMessageForwardError(
  targetRole: Role,
  messageType: string,
  error: unknown
): void {
  console.error(
    `[SignalingSession] Failed to send message to ${targetRole}:`,
    error
  );
  console.error(
    `[SignalingSession] Error details - messageType: ${messageType}, targetRole: ${targetRole}, error: ${error}`
  );
}

/**
 * WebSocketがOPENでない場合の警告ログを出力
 */
export function logWebSocketNotOpen(
  targetRole: Role,
  state: number,
  messageType: string,
  hostWsConnected: boolean,
  viewerWsConnected: boolean
): void {
  const stateText = getWebSocketStateText(state);
  console.warn(
    `[SignalingSession] Target WebSocket (${targetRole}) is not OPEN (state: ${stateText}/${state}). Message type: ${messageType}. Message will be dropped.`
  );
  console.warn(
    `[SignalingSession] Current connections - hostWs: ${
      hostWsConnected ? "connected" : "null"
    }, viewerWs: ${viewerWsConnected ? "connected" : "null"}`
  );
}

/**
 * WebSocketがnullの場合の警告ログを出力
 */
export function logWebSocketNull(
  targetRole: Role,
  messageType: string,
  hostWsConnected: boolean,
  viewerWsConnected: boolean
): void {
  console.warn(
    `[SignalingSession] Target WebSocket (${targetRole}) is null. Message type: ${messageType}. Message will be dropped.`
  );
  console.warn(
    `[SignalingSession] Current connections - hostWs: ${
      hostWsConnected ? "connected" : "null"
    }, viewerWs: ${viewerWsConnected ? "connected" : "null"}`
  );
}

/**
 * メッセージ処理エラーログを出力
 */
export function logMessageError(
  fromRole: Role,
  dataLength: number,
  error: unknown
): void {
  console.error(
    "[SignalingSession] Failed to parse or forward message:",
    error
  );
  console.error(
    `[SignalingSession] Error details - fromRole: ${fromRole}, data_length: ${dataLength}, error: ${error}`
  );
  if (error instanceof Error) {
    console.error(`[SignalingSession] Error stack: ${error.stack}`);
  }
}

/**
 * WebSocket接続ログを出力
 */
export function logWebSocketConnection(role: Role): void {
  console.log(`[SignalingSession] Setting up message handler for ${role}`);
}

/**
 * WebSocketメッセージイベントログを出力
 */
export function logWebSocketMessageEvent(
  role: Role,
  dataType: string,
  dataLength: number
): void {
  console.log(
    `[SignalingSession] Message event received from ${role}, data type: ${dataType}, length: ${dataLength}`
  );
}

/**
 * WebSocketクローズログを出力
 */
export function logWebSocketClose(role: Role): void {
  console.log(`[SignalingSession] WebSocket closed for ${role}`);
}

/**
 * WebSocketエラーログを出力
 */
export function logWebSocketError(role: Role, error: unknown): void {
  console.error(`WebSocket error for ${role}:`, error);
}

/**
 * WebSocketメッセージメソッド呼び出しログを出力
 */
export function logWebSocketMessageMethod(
  role: Role,
  messageType: string,
  messageLength: number
): void {
  console.log(
    `[SignalingSession] webSocketMessage called from ${role}, message type: ${messageType}, length: ${messageLength}`
  );
}

/**
 * WebSocketクローズメソッド呼び出しログを出力
 */
export function logWebSocketCloseMethod(
  role: Role | null,
  code: number,
  reason: string,
  wasClean: boolean
): void {
  console.log(
    `[SignalingSession] webSocketClose called for ${
      role || "unknown"
    }, code: ${code}, reason: ${reason}, wasClean: ${wasClean}`
  );
}

/**
 * WebSocketエラーメソッド呼び出しログを出力
 */
export function logWebSocketErrorMethod(role: Role | null, error: unknown): void {
  console.error(
    `[SignalingSession] webSocketError for ${role || "unknown"}:`,
    error
  );
}

/**
 * roleが見つからない場合のエラーログを出力
 * WebSocket Hibernation 対応: attachment 欠損/不正を明示
 */
export function logRoleNotFound(
  hostWsMatch: boolean,
  viewerWsMatch: boolean
): void {
  console.error(
    "[SignalingSession] webSocketMessage: role not found for WebSocket (attachment missing/invalid)"
  );
  console.error(
    `[SignalingSession] This indicates attachment deserialization failed or attachment is invalid. ` +
    `hostWs match: ${hostWsMatch}, viewerWs match: ${viewerWsMatch}`
  );
}

/**
 * handleMessage呼び出しログを出力
 */
export function logHandleMessage(fromRole: Role, dataLength: number): void {
  console.log(
    `[SignalingSession] handleMessage called from ${fromRole}, data length: ${dataLength}`
  );
}

