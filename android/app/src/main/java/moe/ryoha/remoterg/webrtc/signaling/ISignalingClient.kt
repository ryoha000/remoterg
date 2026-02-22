package moe.ryoha.remoterg.webrtc.signaling

import kotlinx.coroutines.flow.SharedFlow

/**
 * シグナリングサーバーとの通信を管理するインターフェース
 *
 * テスト時は Fake 実装に差し替えることで、実際の WebSocket 接続なしに
 * ViewModel のユニットテストが可能になる。
 */
interface ISignalingClient {
    val messages: SharedFlow<IncomingMessage>
    fun connect(url: String, onConnected: (() -> Unit)? = null)
    fun sendOffer(sdp: String, codec: String = "h264")
    fun sendAnswer(sdp: String)
    fun sendIceCandidate(candidate: String, sdpMid: String, sdpMLineIndex: Int)
    fun disconnect()
}
