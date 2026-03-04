package moe.ryoha.remoterg.webrtc

import android.content.Context
import android.media.AudioManager
import android.util.Log
import dagger.hilt.android.qualifiers.ApplicationContext
import org.webrtc.audio.JavaAudioDeviceModule
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import org.webrtc.DataChannel
import org.webrtc.DefaultVideoDecoderFactory
import org.webrtc.DefaultVideoEncoderFactory
import org.webrtc.RTCStatsCollectorCallback
import org.webrtc.RTCStatsReport
import org.webrtc.EglBase
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpReceiver
import org.webrtc.RtpTransceiver
import org.webrtc.SessionDescription
import org.webrtc.AudioTrack
import org.webrtc.VideoSink
import org.webrtc.VideoTrack
import org.json.JSONObject
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.roundToInt
import javax.inject.Inject
import javax.inject.Singleton

/**
 * WebRTC 接続を管理するマネージャー
 *
 * RN の useViewerConnection.ts と同等の接続フローを実装:
 * 1. init(context) で PeerConnectionFactory を初期化
 * 2. createPeerConnection() で PeerConnection を作成（Offer は自動生成しない）
 * 3. setupConnection() で recvonly transceiver 追加 → DataChannel 作成 → Offer 生成
 */
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context
) : IWebRtcManager {

    private val rootEglBase: EglBase = EglBase.create()
    override val eglBaseContext: EglBase.Context get() = rootEglBase.eglBaseContext

    private var peerConnectionFactory: PeerConnectionFactory? = null
    private var peerConnection: PeerConnection? = null
    private var dataChannel: DataChannel? = null

    // シグナリングイベント（コールバック → SharedFlow）
    private val _localOfferCreated = MutableSharedFlow<String>(extraBufferCapacity = 1)
    override val localOfferCreated: SharedFlow<String> = _localOfferCreated.asSharedFlow()

    private val _localAnswerCreated = MutableSharedFlow<String>(extraBufferCapacity = 1)
    override val localAnswerCreated: SharedFlow<String> = _localAnswerCreated.asSharedFlow()

    private val _iceCandidateCreated = MutableSharedFlow<IceCandidate>(extraBufferCapacity = 10)
    override val iceCandidateCreated: SharedFlow<IceCandidate> = _iceCandidateCreated.asSharedFlow()

    private val _remoteVideoTrack = MutableStateFlow<VideoTrack?>(null)
    override val remoteVideoTrack: StateFlow<VideoTrack?> = _remoteVideoTrack.asStateFlow()

    private var remoteAudioTrack: AudioTrack? = null

    private val _isConnected = MutableStateFlow(false)
    override val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

    private val _iceConnectionState = MutableStateFlow("NEW")
    override val iceConnectionState: StateFlow<String> = _iceConnectionState.asStateFlow()

    private val _signalingState = MutableStateFlow("NEW")
    override val signalingState: StateFlow<String> = _signalingState.asStateFlow()

    private val _dataChannelMessages = MutableSharedFlow<DataChannelMessage>(extraBufferCapacity = 8192)
    override val dataChannelMessages: SharedFlow<DataChannelMessage> = _dataChannelMessages.asSharedFlow()

    private val scope = CoroutineScope(Dispatchers.IO)

    private val _rtcStats = MutableStateFlow(WebRtcStats())
    override val rtcStats: StateFlow<WebRtcStats> = _rtcStats.asStateFlow()

    private var statsJob: kotlinx.coroutines.Job? = null
    private var lastBytesReceived = 0L
    private var lastTimestampUs = 0.0

    private val iceCandidateLock = Any()
    private var pendingIceCandidates = mutableListOf<IceCandidate>()
    private var isRemoteDescriptionSet = false

    private var preferredCodec: String = "h264"

    // E2E latency measurement (LATENCY_MEASUREMENT.md)
    private val latencyOffsetEst = AtomicReference<Double?>(null)
    private val lastLatencyMs = AtomicInteger(0)
    private val frameSampleQueue = ConcurrentLinkedQueue<FrameSample>()
    private data class FrameSample(val tCap: Double, val tEncIn: Double, val tEncOut: Double, val tSend: Double)
    private var syncSeq = 0
    private val syncSamples = mutableListOf<SyncSample>()
    private data class SyncSample(val rtt: Double, val offset: Double)
    private var latencySyncJob: kotlinx.coroutines.Job? = null
    private val latencyVideoSink: VideoSink by lazy { createLatencyVideoSink() }
    
    // WebRTCのRTP timestampNsとtCapの紐付け用オフセット
    private var timestampToTCapOffsetNs: Double? = null

    init {
        scope.launch {
            _isConnected.collect { connected ->
                if (connected) {
                    startStatsPolling()
                    startLatencySync()
                } else {
                    stopStatsPolling()
                    stopLatencySync()
                }
            }
        }
    }

    private fun startLatencySync() {
        latencySyncJob?.cancel()
        syncSeq = 0
        latencySyncJob = scope.launch {
            while (_isConnected.value) {
                syncSeq++
                val c1 = android.os.SystemClock.elapsedRealtimeNanos() / 1_000_000.0
                val req = """{"sync_req":{"seq":$syncSeq,"c1":$c1}}"""
                sendDataChannelMessage(req)
                kotlinx.coroutines.delay(5000)
            }
        }
    }

    private fun stopLatencySync() {
        latencySyncJob?.cancel()
        latencySyncJob = null
        latencyOffsetEst.set(null)
        syncSamples.clear()
        frameSampleQueue.clear()
        lastLatencyMs.set(0)
        timestampToTCapOffsetNs = null
    }

    private fun handleSyncRes(seq: Int, c1: Double, s2: Double, s3: Double) {
        val c4 = android.os.SystemClock.elapsedRealtimeNanos() / 1_000_000.0
        val rtt = (c4 - c1) - (s3 - s2)
        val offset = ((s2 - c1) + (s3 - c4)) / 2.0
        synchronized(syncSamples) {
            syncSamples.add(SyncSample(rtt, offset))
            if (syncSamples.size > 100) syncSamples.removeAt(0)
            val sorted = syncSamples.sortedBy { it.rtt }
            val topCount = (sorted.size * 0.25).toInt().coerceAtLeast(1)
            val adopted = sorted.take(topCount)
            val medianOffset = adopted.map { it.offset }.sorted().let {
                if (it.isEmpty()) 0.0 else it[it.size / 2]
            }
            val old = latencyOffsetEst.get()
            val alpha = 0.1
            val newEst = if (old == null) medianOffset else alpha * medianOffset + (1 - alpha) * old
            latencyOffsetEst.set(newEst)
        }
    }

    private fun handleFrameSample(frameId: Long, tCap: Double, tEncIn: Double, tEncOut: Double, tSend: Double) {
        frameSampleQueue.offer(FrameSample(tCap, tEncIn, tEncOut, tSend))
        while (frameSampleQueue.size > 60) frameSampleQueue.poll()
    }

    private fun onFrameRendered(frame: org.webrtc.VideoFrame) {
        val clockOffset = latencyOffsetEst.get() ?: return
        if (frameSampleQueue.isEmpty()) return

        val frameTimestampMs = frame.timestampNs / 1_000_000.0

        var bestSample: FrameSample? = null
        var minDiff = Double.MAX_VALUE
        var matchedIndex = -1

        val samples = frameSampleQueue.toList()
        for ((index, sample) in samples.withIndex()) {
            val currentOffset = timestampToTCapOffsetNs
            // 初回、またはズレが500ms以上ある場合はオフセットを再計算
            if (currentOffset == null || kotlin.math.abs((frameTimestampMs - currentOffset) - sample.tCap) > 500.0) {
                timestampToTCapOffsetNs = frameTimestampMs - sample.tCap
            }
            
            val predictedTCap = frameTimestampMs - (timestampToTCapOffsetNs ?: 0.0)
            val diff = kotlin.math.abs(predictedTCap - sample.tCap)
            
            // 50ms以内で最も近いものを探す
            if (diff < 50.0 && diff < minDiff) {
                minDiff = diff
                bestSample = sample
                matchedIndex = index
            }
        }

        if (bestSample != null && matchedIndex >= 0) {
            // マッチしたサンプルまでの古いデータをすべて破棄（コマ落ちした未描画フレーム分をクリア）
            for (i in 0..matchedIndex) {
                frameSampleQueue.poll()
            }
            
            val tRender = android.os.SystemClock.elapsedRealtimeNanos() / 1_000_000.0
            val tCapClient = bestSample.tCap - clockOffset
            val e2eMs = (tRender - tCapClient).roundToInt()
            lastLatencyMs.set(e2eMs.coerceIn(0, 9999))
        }
    }

    private fun createLatencyVideoSink(): VideoSink = VideoSink { frame -> onFrameRendered(frame) }

    private fun handleLatencyMessage(text: String): Boolean {
        return try {
            val root = JSONObject(text)
            when {
                root.has("sync_res") -> {
                    val o = root.getJSONObject("sync_res")
                    handleSyncRes(
                        o.getInt("seq"),
                        o.getDouble("c1"),
                        o.getDouble("s2"),
                        o.getDouble("s3")
                    )
                    true
                }
                root.has("frame_sample") -> {
                    val o = root.getJSONObject("frame_sample")
                    handleFrameSample(
                        o.getLong("frame_id"),
                        o.getDouble("t_cap"),
                        o.getDouble("t_enc_in"),
                        o.getDouble("t_enc_out"),
                        o.getDouble("t_send")
                    )
                    true
                }
                else -> false
            }
        } catch (_: Exception) {
            false
        }
    }

    private fun startStatsPolling() {
        if (statsJob?.isActive == true) return
        lastBytesReceived = 0L
        lastTimestampUs = 0.0
        
        statsJob = scope.launch {
            while (true) {
                if (_isConnected.value) {
                    peerConnection?.getStats { report ->
                        var currentFps = 0
                        var currentBytes = 0L
                        var currentTimestamp = 0.0
                        var packetsLost = 0L
                        var packetsReceived = 0L
                        var currentFrameWidth = 0
                        var currentFrameHeight = 0
                        
                        report.statsMap.values.forEach { stat ->
                            val isVideo = stat.members["kind"] == "video"
                            if (stat.type == "inbound-rtp" && isVideo) {
                                currentFps = (stat.members["framesPerSecond"] as? Number)?.toInt() ?: 0
                                currentBytes = (stat.members["bytesReceived"] as? Number)?.toLong() ?: 0L
                                currentTimestamp = stat.timestampUs
                                packetsLost = (stat.members["packetsLost"] as? Number)?.toLong() ?: 0L
                                packetsReceived = (stat.members["packetsReceived"] as? Number)?.toLong() ?: 0L
                                currentFrameWidth = (stat.members["frameWidth"] as? Number)?.toInt() ?: 0
                                currentFrameHeight = (stat.members["frameHeight"] as? Number)?.toInt() ?: 0
                            }
                        }
                        
                        var bitrate = 0
                        if (lastTimestampUs > 0 && currentTimestamp > lastTimestampUs) {
                            val durationS = (currentTimestamp - lastTimestampUs) / 1000000.0
                            val bytesDiff = currentBytes - lastBytesReceived
                            bitrate = ((bytesDiff * 8) / durationS / 1000.0).toInt()
                        }
                        
                        val loss = if (packetsReceived + packetsLost > 0) {
                            ((packetsLost.toDouble() / (packetsReceived + packetsLost)) * 100).toInt()
                        } else {
                            0
                        }
                        
                        lastBytesReceived = currentBytes
                        lastTimestampUs = currentTimestamp
                        
                        _rtcStats.value = WebRtcStats(
                            fps = currentFps,
                            bitrate = bitrate,
                            loss = loss,
                            frameWidth = currentFrameWidth,
                            frameHeight = currentFrameHeight,
                            latencyMs = lastLatencyMs.get()
                        )
                    }
                }
                kotlinx.coroutines.delay(1000)
            }
        }
    }

    private fun stopStatsPolling() {
        statsJob?.cancel()
        statsJob = null
        _rtcStats.value = WebRtcStats(latencyMs = lastLatencyMs.get())
    }

    /**
     * PeerConnectionFactory を初期化する
     * 他のメソッドを呼ぶ前に必ず一度呼ぶこと
     */
    override fun init(context: Context) {
        if (peerConnectionFactory != null) {
            Log.d(TAG, "PeerConnectionFactory は既に初期化されています")
            return
        }

        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(context)
                .setEnableInternalTracer(true)
                .createInitializationOptions()
        )

        // 音声デバイスモジュールを初期化（受信音声の再生に必要）
        val audioAttributes = android.media.AudioAttributes.Builder()
            .setUsage(android.media.AudioAttributes.USAGE_MEDIA)
            .setContentType(android.media.AudioAttributes.CONTENT_TYPE_MOVIE)
            .build()
            
        val audioDeviceModule = JavaAudioDeviceModule.builder(context)
            .setAudioAttributes(audioAttributes)
            .setUseHardwareAcousticEchoCanceler(false)
            .setUseHardwareNoiseSuppressor(false)
            .createAudioDeviceModule()

        val encoderFactory = DefaultVideoEncoderFactory(rootEglBase.eglBaseContext, true, true)
        val decoderFactory = DefaultVideoDecoderFactory(rootEglBase.eglBaseContext)

        peerConnectionFactory = PeerConnectionFactory.builder()
            .setAudioDeviceModule(audioDeviceModule)
            .setVideoEncoderFactory(encoderFactory)
            .setVideoDecoderFactory(decoderFactory)
            .createPeerConnectionFactory()

        // AudioDeviceModule のリソース解放（Factory に渡した後は不要）
        audioDeviceModule.release()

        Log.d(TAG, "PeerConnectionFactory を初期化しました（AudioDeviceModule 設定済み）")
    }

    /**
     * PeerConnection を作成する
     * onRenegotiationNeeded では Offer を自動生成しない（setupConnection で明示的に行う）
     */
    override fun createPeerConnection() {
        Log.d(TAG, "PeerConnection を作成中")

        synchronized(iceCandidateLock) {
            isRemoteDescriptionSet = false
            pendingIceCandidates.clear()
        }

        val rtcConfig = PeerConnection.RTCConfiguration(
            listOf(
                PeerConnection.IceServer.builder("stun:stun.l.google.com:19302").createIceServer(),
                PeerConnection.IceServer.builder("stun:stun1.l.google.com:19302").createIceServer(),
                PeerConnection.IceServer.builder("stun:stun.cloudflare.com:3478").createIceServer()
            )
        ).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            iceConnectionReceivingTimeout = 10000 // ICE check timeout extended to 10s to alleviate trickle ICE race conditions
            iceCheckMinInterval = 500             // Increase frequency of ICE checks
        }

        peerConnection = peerConnectionFactory?.createPeerConnection(rtcConfig, object : PeerConnection.Observer {
            override fun onSignalingChange(state: PeerConnection.SignalingState?) {
                Log.d(TAG, "シグナリング状態変化: $state")
                _signalingState.value = state?.name ?: "UNKNOWN"
            }

            override fun onIceConnectionChange(state: PeerConnection.IceConnectionState?) {
                Log.d(TAG, "ICE 接続状態変化: $state")
                _iceConnectionState.value = state?.name ?: "UNKNOWN"
                _isConnected.value = state == PeerConnection.IceConnectionState.CONNECTED ||
                                     state == PeerConnection.IceConnectionState.COMPLETED

                if (_isConnected.value) {
                    val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
                    audioManager.mode = AudioManager.MODE_NORMAL
                    audioManager.isSpeakerphoneOn = false
                    Log.d(TAG, "AudioManagerを通常モードに切り替えました (メディア音量調整用)")
                }
            }

            override fun onIceConnectionReceivingChange(receiving: Boolean) {}
            override fun onIceGatheringChange(state: PeerConnection.IceGatheringState?) {
                Log.d(TAG, "ICE 収集状態変化: $state")
            }

            override fun onIceCandidate(candidate: IceCandidate?) {
                candidate?.let {
                    Log.d(TAG, "ローカル ICE Candidate 生成: ${it.sdp}")
                    _iceCandidateCreated.tryEmit(it)
                }
            }

            override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>?) {}

            override fun onAddStream(stream: MediaStream?) {
                // deprecated — onTrack を使用するが、フォールバックとして残す
                val videoTrack = stream?.videoTracks?.firstOrNull()
                if (videoTrack != null) {
                    Log.d(TAG, "onAddStream からリモート映像トラックを取得")
                    _remoteVideoTrack.value = videoTrack
                }
                val audioTrack = stream?.audioTracks?.firstOrNull()
                if (audioTrack != null) {
                    Log.d(TAG, "onAddStream からリモート音声トラックを取得")
                    remoteAudioTrack = audioTrack
                }
            }

            override fun onRemoveStream(stream: MediaStream?) {}

            override fun onDataChannel(channel: DataChannel?) {
                Log.d(TAG, "DataChannel 受信: ${channel?.label()}")
                dataChannel = channel
                setupDataChannelListeners()
            }

            override fun onRenegotiationNeeded() {
                // setupConnection() で明示的に Offer を生成するため、ここでは何もしない
                Log.d(TAG, "onRenegotiationNeeded（無視 — setupConnection で Offer を生成）")
            }

            override fun onTrack(transceiver: RtpTransceiver?) {
                val track = transceiver?.receiver?.track()
                if (track is VideoTrack) {
                    Log.d(TAG, "onTrack からリモート映像トラックを取得: ${track.id()}")
                    track.setEnabled(true)
                    track.addSink(latencyVideoSink)
                    _remoteVideoTrack.value = track
                } else if (track is AudioTrack) {
                    Log.d(TAG, "onTrack からリモート音声トラックを取得: ${track.id()}")
                    track.setEnabled(true)
                    remoteAudioTrack = track
                } else {
                    Log.d(TAG, "onTrack: 不明なトラック種別 (${track?.kind()})")
                }
            }
        })
    }

    /**
     * 接続をセットアップする（RN の useViewerConnection の流れに準拠）
     * 1. recvonly transceiver を追加（映像・音声）
     * 2. DataChannel を作成
     * 3. Offer を生成して送信
     */
    override fun setupConnection(codec: String) {
        Log.d(TAG, "接続セットアップ開始 (codec: $codec)")
        this.preferredCodec = codec

        // 1. recvonly transceiver を追加
        peerConnection?.addTransceiver(
            org.webrtc.MediaStreamTrack.MediaType.MEDIA_TYPE_VIDEO,
            RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.RECV_ONLY)
        )
        peerConnection?.addTransceiver(
            org.webrtc.MediaStreamTrack.MediaType.MEDIA_TYPE_AUDIO,
            RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.RECV_ONLY)
        )
        Log.d(TAG, "recvonly transceiver を追加しました")

        // 2. DataChannel を作成
        createDataChannel()

        // 3. Offer を生成
        createOffer()
    }

    private fun createDataChannel() {
        val dcInit = DataChannel.Init()
        dataChannel = peerConnection?.createDataChannel("data", dcInit)
        setupDataChannelListeners()
        Log.d(TAG, "DataChannel を作成しました")
    }

    private fun setupDataChannelListeners() {
        dataChannel?.registerObserver(object : DataChannel.Observer {
            override fun onBufferedAmountChange(amount: Long) {}
            override fun onStateChange() {
                Log.d(TAG, "DataChannel 状態変化: ${dataChannel?.state()}")
            }

            override fun onMessage(buffer: DataChannel.Buffer?) {
                if (buffer != null) {
                    val remaining = buffer.data.remaining()
                    val data = ByteArray(remaining)
                    buffer.data.get(data)

                    if (!buffer.binary) {
                        val text = String(data)
                        if (handleLatencyMessage(text)) return
                    }

                    val msg = if (buffer.binary) {
                        Log.d(TAG, "DataChannel バイナリメッセージ受信: $remaining bytes")
                        DataChannelMessage.Binary(data)
                    } else {
                        val text = String(data)
                        Log.d(TAG, "DataChannel テキストメッセージ受信: ${text.take(100)}")
                        DataChannelMessage.Text(text)
                    }
                    val emitted = _dataChannelMessages.tryEmit(msg)
                    if (!emitted) {
                        Log.e(TAG, "DataChannel メッセージの emit に失敗しました (buffer full)")
                    }
                }
            }
        })
    }

    override fun sendDataChannelMessage(message: String) {
        val buffer = DataChannel.Buffer(java.nio.ByteBuffer.wrap(message.toByteArray()), false)
        dataChannel?.send(buffer)
    }

    private fun createOffer() {
        val constraints = MediaConstraints().apply {
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", "true"))
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
        }

        peerConnection?.createOffer(object : SimpleSdpObserver() {
            override fun onCreateSuccess(desc: SessionDescription?) {
                desc?.let {
                    val mungedSdpString = SdpUtils.preferCodecSdp(it.description, preferredCodec)
                    val mungedDesc = SessionDescription(it.type, mungedSdpString)
                    
                    peerConnection?.setLocalDescription(SimpleSdpObserver(), mungedDesc)
                    Log.d(TAG, "Offer を作成・設定しました (コーデック優先: $preferredCodec)")
                    _localOfferCreated.tryEmit(mungedDesc.description)
                }
            }
        }, constraints)
    }

    override fun handleRemoteDescription(type: String, sdp: String) {
        val sdpType = SessionDescription.Type.fromCanonicalForm(type.lowercase())
        val description = SessionDescription(sdpType, sdp)
        peerConnection?.setRemoteDescription(object : SimpleSdpObserver() {
            override fun onSetSuccess() {
                Log.d(TAG, "リモートディスクリプション ($type) を設定しました")
                synchronized(iceCandidateLock) {
                    isRemoteDescriptionSet = true
                    pendingIceCandidates.forEach {
                        peerConnection?.addIceCandidate(it)
                    }
                    Log.d(TAG, "キューされたリモート ICE Candidate を追加しました (${pendingIceCandidates.size}件)")
                    pendingIceCandidates.clear()
                }
                
                if (sdpType == SessionDescription.Type.OFFER) {
                    createAnswer()
                }
            }
        }, description)
    }

    private fun createAnswer() {
        val constraints = MediaConstraints()
        peerConnection?.createAnswer(object : SimpleSdpObserver() {
            override fun onCreateSuccess(desc: SessionDescription?) {
                desc?.let {
                    peerConnection?.setLocalDescription(SimpleSdpObserver(), it)
                    Log.d(TAG, "Answer を作成・設定しました")
                    _localAnswerCreated.tryEmit(it.description)
                }
            }
        }, constraints)
    }

    override fun handleIceCandidate(candidate: String, sdpMid: String, sdpMLineIndex: Int) {
        val iceCandidate = IceCandidate(sdpMid, sdpMLineIndex, candidate)
        synchronized(iceCandidateLock) {
            if (isRemoteDescriptionSet) {
                val success = peerConnection?.addIceCandidate(iceCandidate) ?: false
                Log.d(TAG, "リモート ICE Candidate を追加しました (success=$success)")
            } else {
                pendingIceCandidates.add(iceCandidate)
                Log.d(TAG, "リモート ICE Candidate をキューに追加しました (現在 ${pendingIceCandidates.size} 件)")
            }
        }
    }

    /**
     * リモート音声トラックの音量を設定する
     * @param volume 0.0（ミュート）〜 1.0（最大）の範囲
     */
    override fun setAudioVolume(volume: Double) {
        val clamped = volume.coerceIn(0.0, 1.0)
        remoteAudioTrack?.setVolume(clamped)
        Log.d(TAG, "音声ボリュームを設定: ${(clamped * 100).toInt()}%")
    }

    override fun sendMouseClick(x: Float, y: Float, button: String) {
        if (dataChannel?.state() == DataChannel.State.OPEN) {
            val jsonMessage = """{"MouseClick": {"x": $x, "y": $y, "button": "$button"}}"""
            sendDataChannelMessage(jsonMessage)
            Log.d(TAG, "マウスクリック送信: $jsonMessage")
        } else {
            Log.w(TAG, "マウスクリック送信失敗: DataChannel が開いていません")
        }
    }

    override fun sendCursorMove(dx: Int, dy: Int) {
        if (dataChannel?.state() == DataChannel.State.OPEN) {
            val jsonMessage = """{"CursorMove": {"dx": $dx, "dy": $dy}}"""
            sendDataChannelMessage(jsonMessage)
        }
    }

    override fun sendCursorClick(button: String) {
        if (dataChannel?.state() == DataChannel.State.OPEN) {
            val jsonMessage = """{"CursorClick": {"button": "$button"}}"""
            sendDataChannelMessage(jsonMessage)
            Log.d(TAG, "カーソルクリック送信: $jsonMessage")
        } else {
            Log.w(TAG, "カーソルクリック送信失敗: DataChannel が開いていません")
        }
    }

    override fun sendKeyEvent(key: String, down: Boolean) {
        if (dataChannel?.state() == DataChannel.State.OPEN) {
            val jsonMessage = """{"Key": {"key": "$key", "down": $down}}"""
            sendDataChannelMessage(jsonMessage)
            Log.d(TAG, "キーイベント送信: $jsonMessage")
        } else {
            Log.w(TAG, "キーイベント送信失敗: DataChannel が開いていません")
        }
    }

    override fun close() {
        stopLatencySync()
        try {
            _remoteVideoTrack.value?.removeSink(latencyVideoSink)
        } catch (_: Exception) {}
        dataChannel?.dispose()
        dataChannel = null

        peerConnection?.dispose()
        peerConnection = null

        // factory はシングルトンのため破棄しない（再利用および SIGSEGV 防止）
        // peerConnectionFactory?.dispose()

        remoteAudioTrack = null
        _remoteVideoTrack.value = null
        _isConnected.value = false
        synchronized(iceCandidateLock) {
            isRemoteDescriptionSet = false
            pendingIceCandidates.clear()
        }
        Log.d(TAG, "WebRtcManager を閉じました")
    }

    companion object {
        private const val TAG = "WebRtcManager"
    }
}

data class WebRtcStats(
    val fps: Int = 0,
    val bitrate: Int = 0,
    val loss: Int = 0,
    val frameWidth: Int = 0,
    val frameHeight: Int = 0,
    val latencyMs: Int = 0
)
