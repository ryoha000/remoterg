package moe.ryoha.remoterg.webrtc.signaling

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * シグナリングサーバーのプロトコルに準拠したメッセージ型定義
 *
 * サーバーは JSON メッセージをそのまま相手側 WebSocket に転送する。
 * フィールド名はサーバー側の snake_case 規約に合わせる。
 */

// --- 送信用メッセージ ---

@Serializable
data class OfferMessage(
    val type: String = "offer",
    val sdp: String,
    val codec: String = "h264"
)

@Serializable
data class AnswerMessage(
    val type: String = "answer",
    val sdp: String
)

@Serializable
data class IceCandidateMessage(
    val type: String = "ice_candidate",
    val candidate: String,
    @SerialName("sdp_mid")
    val sdpMid: String,
    @SerialName("sdp_mline_index")
    val sdpMLineIndex: Int
)

// --- 受信用メッセージ（サーバーから転送される任意のメッセージをパース） ---

@Serializable
data class IncomingMessage(
    val type: String,
    val sdp: String? = null,
    val candidate: String? = null,
    @SerialName("sdp_mid")
    val sdpMid: String? = null,
    @SerialName("sdp_mline_index")
    val sdpMLineIndex: Int? = null,
    @SerialName("session_id")
    val sessionId: String? = null,
    @SerialName("negotiation_id")
    val negotiationId: String? = null,
    val codec: String? = null
)
