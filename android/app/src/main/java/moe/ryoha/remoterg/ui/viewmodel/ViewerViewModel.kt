package moe.ryoha.remoterg.ui.viewmodel

import android.app.Application
import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import moe.ryoha.remoterg.domain.ScreenshotProcessor
import moe.ryoha.remoterg.webrtc.WebRtcManager
import moe.ryoha.remoterg.webrtc.WebRtcStats
import moe.ryoha.remoterg.webrtc.signaling.SignalingClient
import javax.inject.Inject

/**
 * Viewer画面の ViewModel
 *
 * 接続フロー:
 * 1. webRtcManager.init(context) — PeerConnectionFactory 初期化
 * 2. webRtcManager.createPeerConnection() — PeerConnection 作成
 * 3. signalingClient.connect(url) — WebSocket 接続
 * 4. webRtcManager.setupConnection() — recvonly transceiver + DataChannel + Offer 生成
 */
@HiltViewModel
class ViewerViewModel @Inject constructor(
    val webRtcManager: WebRtcManager,
    private val signalingClient: SignalingClient,
    private val screenshotProcessor: ScreenshotProcessor,
    private val application: Application
) : ViewModel() {

    private val _connectionState = MutableStateFlow("Disconnected")
    val connectionState: StateFlow<String> = _connectionState.asStateFlow()

    // 選択された codec
    private val _selectedCodec = MutableStateFlow("h264")
    val selectedCodec: StateFlow<String> = _selectedCodec.asStateFlow()
    
    val rtcStats: StateFlow<WebRtcStats> = webRtcManager.rtcStats

    val screenshotSavedFlow = screenshotProcessor.onScreenshotSaved

    private val _screenshotTriggerFlow = kotlinx.coroutines.flow.MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val screenshotTriggerFlow = _screenshotTriggerFlow.asSharedFlow()

    init {
        setupSignaling()
        setupWebRtcCallbacks()
        screenshotProcessor.startObserving()
    }

    /**
     * シグナリングサーバーからのメッセージを処理する
     * answer → リモートディスクリプション設定
     * ice_candidate → ICE Candidate 追加
     */
    private fun setupSignaling() {
        viewModelScope.launch {
            signalingClient.messages.collect { msg ->
                Log.d(TAG, "シグナリングメッセージ受信: type=${msg.type}")
                when (msg.type) {
                    "offer" -> msg.sdp?.let {
                        webRtcManager.handleRemoteDescription("offer", it)
                    }
                    "answer" -> msg.sdp?.let {
                        webRtcManager.handleRemoteDescription("answer", it)
                        _connectionState.value = "Remote Description Set"
                    }
                    "ice_candidate" -> {
                        if (msg.candidate != null && msg.sdpMid != null && msg.sdpMLineIndex != null) {
                            webRtcManager.handleIceCandidate(msg.candidate, msg.sdpMid, msg.sdpMLineIndex)
                        }
                    }
                }
            }
        }
    }

    /**
     * WebRTC コールバックをシグナリングクライアントに接続する
     */
    private fun setupWebRtcCallbacks() {
        webRtcManager.onLocalOfferCreated = { sdp ->
            Log.d(TAG, "Offer を送信中 (codec=${selectedCodec.value})")
            signalingClient.sendOffer(sdp, selectedCodec.value)
        }
        webRtcManager.onLocalAnswerCreated = { sdp ->
            Log.d(TAG, "Answer を送信中")
            signalingClient.sendAnswer(sdp)
        }
        webRtcManager.onIceCandidateCreated = { candidate ->
            signalingClient.sendIceCandidate(
                candidate = candidate.sdp,
                sdpMid = candidate.sdpMid,
                sdpMLineIndex = candidate.sdpMLineIndex
            )
        }
    }

    private var hasAttemptedConnection = false

    /**
     * ホストに接続する
     * URL は session_id と role を含むシグナリングサーバーの WebSocket URL
     */
    fun connectToHost(url: String, codec: String = "h264") {
        if (hasAttemptedConnection) return
        hasAttemptedConnection = true

        _connectionState.value = "Connecting..."
        _selectedCodec.value = codec

        // 1. PeerConnectionFactory を初期化
        webRtcManager.init(application.applicationContext)

        // 2. PeerConnection を作成
        webRtcManager.createPeerConnection()

        // 3. シグナリングサーバーに接続し、接続完了後に Offer 生成を開始
        //    RN の ws.onopen → setupPeerConnection と同じ流れ
        signalingClient.connect(url) {
            // 4. WS 接続完了 → 接続セットアップ（recvonly transceiver + DataChannel + Offer 生成）
            Log.d(TAG, "WebSocket 接続完了 — 接続セットアップ開始 (codec: ${selectedCodec.value})")
            webRtcManager.setupConnection(selectedCodec.value)
        }
    }

    fun disconnect() {
        signalingClient.disconnect()
        webRtcManager.close()
        _connectionState.value = "Disconnected"
    }

    fun takeScreenshot() {
        screenshotProcessor.requestScreenshot()
        _screenshotTriggerFlow.tryEmit(Unit)
    }

    override fun onCleared() {
        super.onCleared()
        screenshotProcessor.stopObserving()
        disconnect()
    }

    companion object {
        private const val TAG = "ViewerViewModel"
    }
}
