/**
 * メッセージ処理の純粋関数
 */

import type { Role, SessionState, SignalingMessage, EnrichedMessage } from "./types";
import {
  getTargetWebSocket,
  getTargetRole,
} from "./session-state";
import {
  logMessageReceived,
  logWebSocketState,
  logMessageForwarded,
  logMessageForwardError,
  logWebSocketNotOpen,
  logWebSocketNull,
  logMessageError,
  logHandleMessage,
} from "./logger";

/**
 * JSONメッセージをパース
 */
export function parseMessage(data: string): SignalingMessage {
  return JSON.parse(data);
}

/**
 * メッセージにsession_idとnegotiation_idを追加
 */
export function enrichMessage(
  message: SignalingMessage,
  sessionId: string
): EnrichedMessage {
  return {
    ...message,
    session_id: sessionId,
    negotiation_id: message.negotiation_id || "default",
  };
}

/**
 * メッセージを転送
 */
export function forwardMessage(
  state: SessionState,
  fromRole: Role,
  enrichedMessage: EnrichedMessage
): void {
  const targetRole = getTargetRole(fromRole);
  const targetWs = getTargetWebSocket(state, fromRole);

  if (!targetWs) {
    logWebSocketNull(
      targetRole,
      enrichedMessage.type || "unknown",
      state.hostWs !== null,
      state.viewerWs !== null
    );
    return;
  }

  const targetState = targetWs.readyState;
  logWebSocketState(
    targetRole,
    targetState,
    enrichedMessage.type || "unknown"
  );

  if (targetState === WebSocket.OPEN) {
    try {
      const messageJson = JSON.stringify(enrichedMessage);
      targetWs.send(messageJson);
      logMessageForwarded(
        fromRole,
        targetRole,
        enrichedMessage.type || "unknown",
        messageJson.length
      );
    } catch (sendError) {
      logMessageForwardError(
        targetRole,
        enrichedMessage.type || "unknown",
        sendError
      );
    }
  } else {
    logWebSocketNotOpen(
      targetRole,
      targetState,
      enrichedMessage.type || "unknown",
      state.hostWs !== null,
      state.viewerWs !== null
    );
  }
}

/**
 * メッセージ処理のメイン関数
 */
export function handleMessage(
  state: SessionState,
  fromRole: Role,
  data: string
): void {
  logHandleMessage(fromRole, data?.length || 0);

  try {
    const message = parseMessage(data);
    const enrichedMessage = enrichMessage(message, state.sessionId);
    logMessageReceived(fromRole, message, state.sessionId);
    forwardMessage(state, fromRole, enrichedMessage);
  } catch (error) {
    logMessageError(fromRole, data?.length || 0, error);
  }
}

