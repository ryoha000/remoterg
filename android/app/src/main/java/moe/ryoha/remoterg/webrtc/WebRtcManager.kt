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
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
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
    private val nativeLatencyLastUpdateElapsedMs = AtomicLong(0L)
    private val lastNativeCaptureUnixMs = AtomicLong(0L)
    private val frameSampleQueue = ConcurrentLinkedQueue<FrameSample>()
    private data class FrameSample(
        val seq: Long,
        val frameId: Long,
        val tCap: Double,
        val tEncIn: Double,
        val tEncOut: Double,
        val tSend: Double,
        val captureUnixMs: Long,
        val receivedUnixMs: Long,
        val receivedElapsedMs: Long
    )
    private data class NativeCaptureSample(
        val timestampUs: Long,
        val captureUnixMs: Long,
        val receivedElapsedMs: Long
    )
    private data class RenderFrameSample(
        val timestampUs: Long,
        val renderUnixMs: Long,
        val receivedElapsedMs: Long
    )
    private class NativeRenderMatchStore(
        private val ttlMs: Long = 1_000L,
        private val logTag: String = "WebRtcManager"
    ) {
        data class MatchedCapture(
            val timestampUs: Long,
            val captureUnixMs: Long,
            val renderUnixMs: Long,
            val nativePendingAfterMatch: Int,
            val renderPendingAfterMatch: Int
        )

        private val nativeByTimestamp = HashMap<Long, NativeCaptureSample>()
        private val renderByTimestamp = HashMap<Long, RenderFrameSample>()

        @Synchronized
        fun offerNative(sample: NativeCaptureSample): MatchedCapture? {
            trimLocked(sample.receivedElapsedMs)
            val pendingRender = renderByTimestamp.remove(sample.timestampUs)
            if (pendingRender != null) {
                return MatchedCapture(
                    timestampUs = sample.timestampUs,
                    captureUnixMs = sample.captureUnixMs,
                    renderUnixMs = pendingRender.renderUnixMs,
                    nativePendingAfterMatch = nativeByTimestamp.size,
                    renderPendingAfterMatch = renderByTimestamp.size
                )
            }
            nativeByTimestamp[sample.timestampUs] = sample
            return null
        }

        @Synchronized
        fun offerRender(sample: RenderFrameSample): MatchedCapture? {
            trimLocked(sample.receivedElapsedMs)
            val pendingNative = nativeByTimestamp.remove(sample.timestampUs)
            if (pendingNative != null) {
                return MatchedCapture(
                    timestampUs = sample.timestampUs,
                    captureUnixMs = pendingNative.captureUnixMs,
                    renderUnixMs = sample.renderUnixMs,
                    nativePendingAfterMatch = nativeByTimestamp.size,
                    renderPendingAfterMatch = renderByTimestamp.size
                )
            }
            renderByTimestamp[sample.timestampUs] = sample
            return null
        }

        @Synchronized
        fun nativePendingCount(): Int = nativeByTimestamp.size

        @Synchronized
        fun renderPendingCount(): Int = renderByTimestamp.size

        @Synchronized
        fun clear() {
            if (nativeByTimestamp.isNotEmpty() || renderByTimestamp.isNotEmpty()) {
                Log.w(
                    logTag,
                    "Latency[C-clear]: drop pending entries native=${nativeByTimestamp.size} render=${renderByTimestamp.size}"
                )
            }
            nativeByTimestamp.clear()
            renderByTimestamp.clear()
        }

        private fun trimLocked(nowElapsedMs: Long) {
            var evictedNative = 0
            var evictedRender = 0
            nativeByTimestamp.entries.removeIf {
                val expired = nowElapsedMs - it.value.receivedElapsedMs > ttlMs
                if (expired) evictedNative++
                expired
            }
            renderByTimestamp.entries.removeIf {
                val expired = nowElapsedMs - it.value.receivedElapsedMs > ttlMs
                if (expired) evictedRender++
                expired
            }
            if (evictedNative > 0 || evictedRender > 0) {
                Log.w(
                    logTag,
                    "Latency[C-evict]: evictedNative=$evictedNative evictedRender=$evictedRender nativePending=${nativeByTimestamp.size} renderPending=${renderByTimestamp.size} ttlMs=$ttlMs"
                )
            }
        }
    }
    private var syncSeq = 0
    private val syncSamples = mutableListOf<SyncSample>()
    private data class SyncSample(val rtt: Double, val offset: Double)
    private var latencySyncJob: kotlinx.coroutines.Job? = null
    private val latencyVideoSink: VideoSink by lazy { createLatencyVideoSink() }
    private val captureTimeStore = CaptureTimeStore()
    private val nativeRenderMatchStore = NativeRenderMatchStore()
    private val latencyNativeSink = LatencyNativeSink()

    // WebRTC の VideoFrame.timestampNs (monotonic) と tCap の紐付け用オフセット（方法B フォールバック用）
    private var timestampToTCapOffsetMs: Double? = null
    private val frameSampleLogCount = AtomicInteger(0)
    private val nativeLogCount = AtomicInteger(0)
    private val nativeMissingLogCount = AtomicInteger(0)
    private val nativeClockAheadEstimateMs = AtomicLong(0L)
    private val nativeClockAheadLastUpdateElapsedMs = AtomicLong(0L)
    private val lastNativeCallbackE2eMs = AtomicInteger(0)
    private val lastNativeCallbackUpdateElapsedMs = AtomicLong(0L)
    private val nativeExtractOkCount = AtomicLong(0L)
    private val nativeExtractFailCount = AtomicLong(0L)

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
        latencyNativeSink.detach()
        nativeLatencyLastUpdateElapsedMs.set(0L)
        lastNativeCaptureUnixMs.set(0L)
        nativeClockAheadEstimateMs.set(0L)
        nativeClockAheadLastUpdateElapsedMs.set(0L)
        lastNativeCallbackE2eMs.set(0)
        lastNativeCallbackUpdateElapsedMs.set(0L)
        latencyOffsetEst.set(null)
        syncSamples.clear()
        frameSampleQueue.clear()
        captureTimeStore.clear()
        nativeRenderMatchStore.clear()
        nativeExtractOkCount.set(0L)
        nativeExtractFailCount.set(0L)
        lastLatencyMs.set(0)
        timestampToTCapOffsetMs = null
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

    private fun handleFrameSample(seq: Long, frameId: Long, tCap: Double, tEncIn: Double, tEncOut: Double, tSend: Double, captureUnixMs: Long) {
        val nowUnixMs = System.currentTimeMillis()
        val nowElapsedMs = android.os.SystemClock.elapsedRealtime()
        frameSampleQueue.offer(
            FrameSample(
                seq = seq,
                frameId = frameId,
                tCap = tCap,
                tEncIn = tEncIn,
                tEncOut = tEncOut,
                tSend = tSend,
                captureUnixMs = captureUnixMs,
                receivedUnixMs = nowUnixMs,
                receivedElapsedMs = nowElapsedMs
            )
        )
        while (frameSampleQueue.size > 60) frameSampleQueue.poll()

        val logCount = frameSampleLogCount.incrementAndGet()
        val msgLagMs = if (captureUnixMs > 0) nowUnixMs - captureUnixMs else Long.MIN_VALUE
        val nativeUpdatedAt = nativeLatencyLastUpdateElapsedMs.get()
        val nativeActive =
            nativeUpdatedAt > 0L && (android.os.SystemClock.elapsedRealtime() - nativeUpdatedAt) in 0..1500L
        if (captureUnixMs > nowUnixMs + 5 && !nativeActive) {
            Log.w(
                TAG,
                "Latency[frame_sample-future]: seq=$seq frameId=$frameId captureUnixMs=$captureUnixMs nowUnixMs=$nowUnixMs msgLagMs=$msgLagMs queue=${frameSampleQueue.size}"
            )
        } else if (logCount % 120 == 1) {
            Log.d(
                TAG,
                "Latency[frame_sample]: seq=$seq frameId=$frameId captureUnixMs=$captureUnixMs nowUnixMs=$nowUnixMs msgLagMs=$msgLagMs queue=${frameSampleQueue.size}"
            )
        }
    }

    private var renderLogCount = 0

    private fun onFrameRendered(frame: org.webrtc.VideoFrame) {
        val tRender = System.currentTimeMillis()
        val frameTimestampUs = frame.timestampNs / 1_000L
        val nowElapsedMs = android.os.SystemClock.elapsedRealtime()
        renderLogCount++
        val shouldLog = renderLogCount % 90 == 1

        // 方法C: Native sink の packet_infos.absolute_capture_time が直近で更新されている間はそれを優先
        val nativeUpdatedAt = nativeLatencyLastUpdateElapsedMs.get()
        if (nativeUpdatedAt == 0L && shouldLog) {
            val count = nativeMissingLogCount.incrementAndGet()
            if (count <= 5 || count % 30 == 1) {
                Log.w(
                    TAG,
                    "Latency[C-missing]: native packet_infos callback not received yet, using fallback path queue=${frameSampleQueue.size}"
                )
            }
        }
        if (nativeUpdatedAt > 0L) {
            val ageMs = nowElapsedMs - nativeUpdatedAt
            if (ageMs in 0..1500L) {
                val matched = nativeRenderMatchStore.offerRender(
                    RenderFrameSample(
                        timestampUs = frameTimestampUs,
                        renderUnixMs = tRender,
                        receivedElapsedMs = nowElapsedMs
                    )
                )
                if (matched != null) {
                    val e2eMs = (matched.renderUnixMs - matched.captureUnixMs).toInt()
                    if (e2eMs < 0) {
                        Log.w(
                            TAG,
                            "Latency[C-negative]: e2e=${e2eMs}ms frameTsUs=${matched.timestampUs} captureUnixMs=${matched.captureUnixMs} tRender=${matched.renderUnixMs} nativePending=${matched.nativePendingAfterMatch} renderPending=${matched.renderPendingAfterMatch} queue=${frameSampleQueue.size}"
                        )
                    } else if (shouldLog) {
                        lastLatencyMs.set(e2eMs.coerceIn(0, 9999))
                        Log.d(
                            TAG,
                            "Latency[C]: e2e=${e2eMs}ms frameTsUs=${matched.timestampUs} captureUnixMs=${matched.captureUnixMs} tRender=${matched.renderUnixMs} nativePending=${matched.nativePendingAfterMatch} renderPending=${matched.renderPendingAfterMatch} queue=${frameSampleQueue.size}"
                        )
                    } else {
                        lastLatencyMs.set(e2eMs.coerceIn(0, 9999))
                    }
                    return
                }
                if (shouldLog) {
                    Log.d(
                        TAG,
                        "Latency[C-miss]: native active but no exact ts match frameTsUs=$frameTimestampUs nativePending=${nativeRenderMatchStore.nativePendingCount()} renderPending=${nativeRenderMatchStore.renderPendingCount()} age=${ageMs}ms fallbackQueue=${frameSampleQueue.size}"
                    )
                }
                val callbackUpdatedAt = lastNativeCallbackUpdateElapsedMs.get()
                if (callbackUpdatedAt > 0L &&
                    nowElapsedMs - callbackUpdatedAt <= 1000L
                ) {
                    val provisionalMs = lastNativeCallbackE2eMs.get()
                    if (provisionalMs > 0) {
                        lastLatencyMs.set(provisionalMs.coerceIn(0, 9999))
                    }
                }
                // Native 経路が生きている間は、不安定な A/B フォールバックで値を上書きしない。
                return
            }
        }

        // 方法A: Decoder の captureTimeNs（EncodedImage から、abs-capture-time 由来の可能性）
        val captureTimeNsFromDecoder = captureTimeStore.pollCaptureTimeNs()
        val captureUnixMsFromDecoder = captureTimeNsFromDecoder?.let { NtpUtils.captureTimeNsToUnixMs(it) }

        if (captureUnixMsFromDecoder != null) {
            val e2eMs = (tRender - captureUnixMsFromDecoder).toInt()
            if (e2eMs >= 0) {
                lastLatencyMs.set(e2eMs.coerceIn(0, 9999))
            }
            frameSampleQueue.poll()
            if (shouldLog) {
                Log.d(
                    TAG,
                    "Latency[A]: e2e=${e2eMs}ms captureTimeNs=$captureTimeNsFromDecoder unixMs=$captureUnixMsFromDecoder tRender=$tRender queue=${frameSampleQueue.size}"
                )
            }
            return
        }

        // 方法B（DataChannel frame_sample）にはフォールバックしない。
        if (shouldLog) {
            Log.d(TAG, "Latency[skip-no-b]: C/A unavailable, keeping last latency value queue=${frameSampleQueue.size}")
        }
    }

    private fun createLatencyVideoSink(): VideoSink = VideoSink { frame -> onFrameRendered(frame) }

    private fun updateNativeClockAheadEstimate(rawMsgLagMs: Long, nowElapsedMs: Long): Long {
        val current = nativeClockAheadEstimateMs.get()
        val lastUpdateMs = nativeClockAheadLastUpdateElapsedMs.get()
        var next = current
        if (rawMsgLagMs < -5L) {
            val observedAheadMs = (-rawMsgLagMs).coerceIn(0L, 500L)
            next = if (current <= 0L || observedAheadMs >= current) {
                // future 側への観測値には素早く追従
                current + ((observedAheadMs - current) * 3L) / 4L
            } else {
                // 小さい観測値にはゆっくり寄せる
                current - ((current - observedAheadMs) / 20L)
            }
        } else if (current > 0L && lastUpdateMs > 0L && nowElapsedMs > lastUpdateMs) {
            // 観測が future でない間は時間ベースでゆっくり減衰 (2ms/s)
            val elapsedMs = nowElapsedMs - lastUpdateMs
            val decay = (elapsedMs / 1000L) * 2L
            if (decay > 0L) {
                next = (current - decay).coerceAtLeast(0L)
            }
        }
        nativeClockAheadEstimateMs.set(next)
        nativeClockAheadLastUpdateElapsedMs.set(nowElapsedMs)
        return next
    }

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
                        o.getLong("seq"),
                        o.getLong("frame_id"),
                        o.getDouble("t_cap"),
                        o.getDouble("t_enc_in"),
                        o.getDouble("t_enc_out"),
                        o.getDouble("t_send"),
                        o.getLong("capture_unix_ms")
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
        val innerDecoderFactory = DefaultVideoDecoderFactory(rootEglBase.eglBaseContext)
        val decoderFactory = LatencyDecoderFactory(innerDecoderFactory, captureTimeStore)

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
                    Log.d(TAG, "Latency[C-attach]: attaching LatencyNativeSink to track id=${track.id()}")
                    latencyNativeSink.attachToTrack(track, LatencyNativeSink.Callback { status, captureUnixMs, timestampUs ->
                        val nowElapsedMs = android.os.SystemClock.elapsedRealtime()
                        nativeLatencyLastUpdateElapsedMs.set(nowElapsedMs)

                        if (status != CAPTURE_STATUS_OK) {
                            val failCount = nativeExtractFailCount.incrementAndGet()
                            if (failCount <= 5 || failCount % 120 == 1L) {
                                Log.w(
                                    TAG,
                                    "Latency[C-native-skip]: status=${captureStatusToLabel(status)}($status) frameTsUs=$timestampUs nativePending=${nativeRenderMatchStore.nativePendingCount()} renderPending=${nativeRenderMatchStore.renderPendingCount()} fail=$failCount ok=${nativeExtractOkCount.get()}"
                                )
                            }
                            return@Callback
                        }

                        nativeExtractOkCount.incrementAndGet()
                        val nowUnixMs = System.currentTimeMillis()
                        val rawMsgLagMs = nowUnixMs - captureUnixMs
                        val aheadEstimateMs = updateNativeClockAheadEstimate(rawMsgLagMs, nowElapsedMs)
                        val correctedCaptureUnixMs = captureUnixMs - aheadEstimateMs
                        val correctedMsgLagMs = nowUnixMs - correctedCaptureUnixMs
                        if (correctedMsgLagMs >= 0L) {
                            lastNativeCallbackE2eMs.set(correctedMsgLagMs.toInt().coerceIn(0, 9999))
                            lastNativeCallbackUpdateElapsedMs.set(nowElapsedMs)
                        }
                        lastNativeCaptureUnixMs.set(correctedCaptureUnixMs)
                        val matched = nativeRenderMatchStore.offerNative(
                            NativeCaptureSample(
                                timestampUs = timestampUs,
                                captureUnixMs = correctedCaptureUnixMs,
                                receivedElapsedMs = nowElapsedMs
                            )
                        )
                        if (matched != null) {
                            val e2eMs = (matched.renderUnixMs - matched.captureUnixMs).toInt()
                            if (e2eMs >= 0) {
                                lastLatencyMs.set(e2eMs.coerceIn(0, 9999))
                            } else {
                                Log.w(
                                    TAG,
                                    "Latency[C-negative-native-first]: e2e=${e2eMs}ms frameTsUs=${matched.timestampUs} captureUnixMs=${matched.captureUnixMs} tRender=${matched.renderUnixMs} nativePending=${matched.nativePendingAfterMatch} renderPending=${matched.renderPendingAfterMatch} queue=${frameSampleQueue.size}"
                                )
                            }
                        }
                        val count = nativeLogCount.incrementAndGet()
                        if (correctedMsgLagMs < -20) {
                            Log.w(
                                TAG,
                                "Latency[C-native-future]: rawCaptureUnixMs=$captureUnixMs correctedCaptureUnixMs=$correctedCaptureUnixMs nowUnixMs=$nowUnixMs rawMsgLagMs=$rawMsgLagMs correctedMsgLagMs=$correctedMsgLagMs aheadEstimateMs=$aheadEstimateMs frameTsUs=$timestampUs nativePending=${nativeRenderMatchStore.nativePendingCount()} renderPending=${nativeRenderMatchStore.renderPendingCount()} queue=${frameSampleQueue.size}"
                            )
                        } else if (count % 120 == 1) {
                            Log.d(
                                TAG,
                                "Latency[C-native]: rawCaptureUnixMs=$captureUnixMs correctedCaptureUnixMs=$correctedCaptureUnixMs nowUnixMs=$nowUnixMs rawMsgLagMs=$rawMsgLagMs correctedMsgLagMs=$correctedMsgLagMs aheadEstimateMs=$aheadEstimateMs frameTsUs=$timestampUs nativePending=${nativeRenderMatchStore.nativePendingCount()} renderPending=${nativeRenderMatchStore.renderPendingCount()} queue=${frameSampleQueue.size}"
                            )
                        }
                    })
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
                    var mungedSdpString = SdpUtils.preferCodecSdp(it.description, preferredCodec)
                    // abs-capture-time 拡張ヘッダーを強制注入
                    mungedSdpString = SdpUtils.addAbsCaptureTimeExtension(mungedSdpString)
                    logAbsCaptureTimeInSdp("local-offer", mungedSdpString)
                    val mungedDesc = SessionDescription(it.type, mungedSdpString)
                    
                    peerConnection?.setLocalDescription(SimpleSdpObserver(), mungedDesc)
                    Log.d(TAG, "Offer を作成・設定しました (コーデック優先: $preferredCodec)")
                    _localOfferCreated.tryEmit(mungedDesc.description)
                }
            }
        }, constraints)
    }

    override fun handleRemoteDescription(type: String, sdp: String) {
        logAbsCaptureTimeInSdp("remote-$type", sdp)
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
        latencyNativeSink.release()
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
        private const val ABS_CAPTURE_TIME_URI =
            "http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time"
        private const val CAPTURE_STATUS_OK = 0
        private const val CAPTURE_STATUS_NO_PACKET_INFOS = 1
        private const val CAPTURE_STATUS_NO_ABS_CAPTURE_TIME = 2
        private const val CAPTURE_STATUS_OUT_OF_RANGE = 3
        private const val CAPTURE_STATUS_NO_LOCAL_CAPTURE_CLOCK_OFFSET = 4
    }

    private fun captureStatusToLabel(status: Int): String {
        return when (status) {
            CAPTURE_STATUS_OK -> "ok"
            CAPTURE_STATUS_NO_PACKET_INFOS -> "no_packet_infos"
            CAPTURE_STATUS_NO_ABS_CAPTURE_TIME -> "no_abs_capture_time"
            CAPTURE_STATUS_OUT_OF_RANGE -> "out_of_range"
            CAPTURE_STATUS_NO_LOCAL_CAPTURE_CLOCK_OFFSET -> "no_local_capture_clock_offset"
            else -> "unknown"
        }
    }

    private fun logAbsCaptureTimeInSdp(label: String, sdp: String) {
        val extmaps = mutableListOf<String>()
        var inVideo = false
        var videoMid: String? = null

        sdp.lines().forEach { raw ->
            val line = raw.trim()
            if (line.startsWith("m=")) {
                inVideo = line.startsWith("m=video")
            }
            if (!inVideo) return@forEach
            if (line.startsWith("a=mid:")) {
                videoMid = line.removePrefix("a=mid:")
            }
            if (line.startsWith("a=extmap:") && line.contains(ABS_CAPTURE_TIME_URI)) {
                extmaps.add(line)
            }
        }

        if (extmaps.isEmpty()) {
            Log.w(TAG, "Latency[ACT-SDP:$label]: video extmap not found")
        } else {
            Log.d(
                TAG,
                "Latency[ACT-SDP:$label]: mid=${videoMid ?: "unknown"} extmaps=${extmaps.joinToString(" | ")}"
            )
        }
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

