package moe.ryoha.remoterg.webrtc

import android.content.Context
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import org.webrtc.EglBase
import org.webrtc.IceCandidate
import org.webrtc.VideoTrack

/**
 * WebRTC 接続を管理するインターフェース
 *
 * テスト時は Fake 実装に差し替えることで、実際の WebRTC ライブラリなしに
 * ViewModel のユニットテストが可能になる。
 */
interface IWebRtcManager {

    // ─── 公開状態 ───

    val remoteVideoTrack: StateFlow<VideoTrack?>
    val isConnected: StateFlow<Boolean>
    val iceConnectionState: StateFlow<String>
    val signalingState: StateFlow<String>
    val rtcStats: StateFlow<WebRtcStats>
    val dataChannelMessages: SharedFlow<DataChannelMessage>
    val eglBaseContext: EglBase.Context

    // ─── シグナリングイベント（コールバック → SharedFlow） ───

    /** ローカル Offer SDP が生成された */
    val localOfferCreated: SharedFlow<String>
    /** ローカル Answer SDP が生成された */
    val localAnswerCreated: SharedFlow<String>
    /** ローカル ICE Candidate が生成された */
    val iceCandidateCreated: SharedFlow<IceCandidate>

    // ─── 操作 ───

    fun init(context: Context)
    fun createPeerConnection()
    fun setupConnection(codec: String = "h264")
    fun handleRemoteDescription(type: String, sdp: String)
    fun handleIceCandidate(candidate: String, sdpMid: String, sdpMLineIndex: Int)
    fun setAudioVolume(volume: Double)
    fun sendDataChannelMessage(message: String)
    fun sendMouseClick(x: Float, y: Float, button: String = "left")
    fun sendCursorMove(dx: Int, dy: Int)
    fun sendCursorClick(button: String)
    fun sendKeyEvent(key: String, down: Boolean)
    fun close()
}
