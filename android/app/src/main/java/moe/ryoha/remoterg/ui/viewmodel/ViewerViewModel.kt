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
import moe.ryoha.remoterg.data.repository.SettingsRepository
import moe.ryoha.remoterg.domain.ScreenshotProcessor
import moe.ryoha.remoterg.webrtc.IWebRtcManager
import moe.ryoha.remoterg.webrtc.WebRtcStats
import moe.ryoha.remoterg.webrtc.signaling.ISignalingClient
import org.webrtc.EglBase
import org.webrtc.VideoTrack
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
    private val webRtcManager: IWebRtcManager,
    private val signalingClient: ISignalingClient,
    private val screenshotProcessor: ScreenshotProcessor,
    private val settingsRepository: SettingsRepository,
    private val application: Application
) : ViewModel() {

    private val _connectionState = MutableStateFlow("Disconnected")
    val connectionState: StateFlow<String> = _connectionState.asStateFlow()

    // 選択された codec
    private val _selectedCodec = MutableStateFlow("h264")
    val selectedCodec: StateFlow<String> = _selectedCodec.asStateFlow()
    
    val rtcStats: StateFlow<WebRtcStats> = webRtcManager.rtcStats

    // Screen に必要な WebRTC 状態を ViewModel から直接公開
    // Screen が webRtcManager を直接参照しないようにするため
    val remoteVideoTrack: StateFlow<VideoTrack?> = webRtcManager.remoteVideoTrack
    val isConnected: StateFlow<Boolean> = webRtcManager.isConnected
    val iceConnectionState: StateFlow<String> = webRtcManager.iceConnectionState
    val signalingState: StateFlow<String> = webRtcManager.signalingState
    val eglBaseContext: EglBase.Context get() = webRtcManager.eglBaseContext
    val webSocketState: StateFlow<String> = signalingClient.webSocketState

    val useOriginalQualityScreenshot: StateFlow<Boolean> = settingsRepository.useOriginalQualityScreenshot
    val isShiftButtonEnabled: StateFlow<Boolean> = settingsRepository.isShiftButtonEnabled
    val isTrackpadModeEnabled: StateFlow<Boolean> = settingsRepository.isTrackpadModeEnabled

    private val _connectionError = MutableStateFlow<String?>(null)
    val connectionError: StateFlow<String?> = _connectionError.asStateFlow()

    val screenshotSavedFlow = screenshotProcessor.onScreenshotSaved

    private val _screenshotTriggerFlow = kotlinx.coroutines.flow.MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val screenshotTriggerFlow = _screenshotTriggerFlow.asSharedFlow()

    private val _localScreenshotTriggerFlow = kotlinx.coroutines.flow.MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val localScreenshotTriggerFlow = _localScreenshotTriggerFlow.asSharedFlow()

    // Host session url that we are currently connecting to
    private var currentSignalingUrl: String? = null

    init {
        setupSignaling()
        setupWebRtcCallbacks()
        setupWebSocketStateTracking()
        setupWebRtcStateTracking()
        screenshotProcessor.startObserving()
    }

    private fun setupWebSocketStateTracking() {
        viewModelScope.launch {
            signalingClient.webSocketState.collect { state ->
                // もし既に意図的に Disconnected な状態であれば、エラー画面に遷移させない
                if (_connectionState.value == "Disconnected") return@collect

                if (state.startsWith("Error")) {
                    _connectionError.value = state.substringAfter("Error: ").trim()
                    _connectionState.value = "Failed"
                } else if (state == "Disconnected" && _connectionState.value != "Failed") {
                    _connectionError.value = "WebSocket Connection Closed"
                    _connectionState.value = "Failed"
                }
            }
        }
    }

    private fun setupWebRtcStateTracking() {
        viewModelScope.launch {
            webRtcManager.iceConnectionState.collect { state ->
                // もし既に意図的に Disconnected な状態であれば、クローズ時の等によるエラー画面遷移を防ぐ
                if (_connectionState.value == "Disconnected") return@collect

                if (state == "FAILED" || state == "CLOSED") {
                    _connectionError.value = "WebRTC Connection $state"
                    _connectionState.value = "Failed"
                } else if (state == "DISCONNECTED" && _connectionState.value != "Failed") {
                    _connectionError.value = "Host Disconnected"
                    _connectionState.value = "Failed"
                }
            }
        }
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
     * WebRTC イベント（SharedFlow）をシグナリングクライアントに接続する
     */
    private fun setupWebRtcCallbacks() {
        viewModelScope.launch {
            webRtcManager.localOfferCreated.collect { sdp ->
                Log.d(TAG, "Offer を送信中 (codec=${selectedCodec.value})")
                signalingClient.sendOffer(sdp, selectedCodec.value)
            }
        }
        viewModelScope.launch {
            webRtcManager.localAnswerCreated.collect { sdp ->
                Log.d(TAG, "Answer を送信中")
                signalingClient.sendAnswer(sdp)
            }
        }
        viewModelScope.launch {
            webRtcManager.iceCandidateCreated.collect { candidate ->
                signalingClient.sendIceCandidate(
                    candidate = candidate.sdp,
                    sdpMid = candidate.sdpMid,
                    sdpMLineIndex = candidate.sdpMLineIndex
                )
            }
        }
    }

    private var hasAttemptedConnection = false

    /**
     * ホストに接続する
     * URL は session_id と role を含むシグナリングサーバーの WebSocket URL
     */
    fun connectToHost(url: String, codec: String = "h264") {
        if (hasAttemptedConnection && _connectionState.value != "Failed" && _connectionState.value != "Disconnected") return

        if (hasAttemptedConnection) {
            // Clean up previous connection completely
            Log.d(TAG, "Retrying connection: cleaning up previous state")
            webRtcManager.close()
        }

        hasAttemptedConnection = true
        _connectionError.value = null

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
        hasAttemptedConnection = false
        _connectionState.value = "Disconnected"
        signalingClient.disconnect()
        webRtcManager.close()
    }

    fun takeScreenshot() {
        val useOriginal = useOriginalQualityScreenshot.value
        screenshotProcessor.requestScreenshot(includeImage = useOriginal)
        
        if (useOriginal) {
            _screenshotTriggerFlow.tryEmit(Unit)
        } else {
            _localScreenshotTriggerFlow.tryEmit(Unit)
        }
    }

    fun saveLocalScreenshot(bitmap: android.graphics.Bitmap) {
        screenshotProcessor.pendingLocalBitmap = bitmap
    }

    /**
     * リモート音声トラックの音量を設定する
     */
    fun setAudioVolume(volume: Double) {
        webRtcManager.setAudioVolume(volume)
    }

    fun setUseOriginalQualityScreenshot(useOriginal: Boolean) {
        settingsRepository.setUseOriginalQualityScreenshot(useOriginal)
    }

    fun setShiftButtonEnabled(enabled: Boolean) {
        settingsRepository.setShiftButtonEnabled(enabled)
    }

    fun setTrackpadModeEnabled(enabled: Boolean) {
        settingsRepository.setTrackpadModeEnabled(enabled)
    }

    fun sendMouseClick(x: Float, y: Float, button: String = "left") {
        webRtcManager.sendMouseClick(x, y, button)
    }

    fun sendCursorMove(dx: Int, dy: Int) {
        webRtcManager.sendCursorMove(dx, dy)
    }

    fun sendCursorClick(button: String = "left") {
        webRtcManager.sendCursorClick(button)
    }

    /**
     * キーイベントを送信する
     * @param key キー名 (例: "Control", "A" など)
     * @param down 押されたか離されたか
     */
    fun sendKeyEvent(key: String, down: Boolean) {
        webRtcManager.sendKeyEvent(key, down)
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
