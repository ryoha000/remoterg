package moe.ryoha.remoterg.webrtc

import org.webrtc.VideoCodecInfo
import org.webrtc.VideoDecoder
import org.webrtc.VideoDecoderFactory
import org.webrtc.WrappedNativeVideoDecoder
import org.webrtc.EncodedImage
import java.util.concurrent.ConcurrentLinkedQueue
import android.util.Log
import org.webrtc.VideoCodecStatus

/**
 * VideoDecoderFactory ラッパー。EncodedImage.captureTimeNs を傍受し、
 * デコード後の VideoFrame.timestampNs と紐付けて CaptureTimeStore に格納する。
 * abs-capture-time RTP 拡張の NTP タイムスタンプを E2E レイテンシ計算に活用する。
 */
class LatencyDecoderFactory(
    private val inner: VideoDecoderFactory,
    private val captureTimeStore: CaptureTimeStore
) : VideoDecoderFactory {

    override fun createDecoder(info: VideoCodecInfo): VideoDecoder? {
        val innerDecoder = inner.createDecoder(info) ?: return null
        if (innerDecoder is WrappedNativeVideoDecoder) {
            Log.d("LatencyDecoderFactory", "Native decoder returned; skipping LatencyVideoDecoder wrap")
            return innerDecoder
        }
        return LatencyVideoDecoder(innerDecoder, captureTimeStore)
    }

    override fun getSupportedCodecs(): Array<VideoCodecInfo> = inner.supportedCodecs
}

/**
 * EncodedImage.captureTimeNs を FIFO で保持するストア。
 * decode() 呼び出し順と onFrameRendered() のフレーム順が一致する前提で、
 * pollCaptureTimeNs() により対応する captureTimeNs を取得する。
 */
class CaptureTimeStore {
    private val captureTimeQueue = ConcurrentLinkedQueue<Long>()

    /** デコード要求時に push。EncodedImage.captureTimeNs を渡す */
    fun pushCaptureTimeNs(captureTimeNs: Long) {
        captureTimeQueue.offer(captureTimeNs)
        while (captureTimeQueue.size > 120) captureTimeQueue.poll()
    }

    /** FIFO から pop。描画フレームと 1:1 対応する captureTimeNs を取得 */
    fun pollCaptureTimeNs(): Long? = captureTimeQueue.poll()

    fun clear() {
        captureTimeQueue.clear()
    }
}

/**
 * VideoDecoder ラッパー。decode(EncodedImage) で captureTimeNs を記録し、
 * onDecodedFrame で VideoFrame.timestampNs と紐付ける。
 */
private class LatencyVideoDecoder(
    private val inner: VideoDecoder,
    private val captureTimeStore: CaptureTimeStore
) : VideoDecoder {

    companion object {
        private const val TAG = "LatencyVideoDecoder"
        private const val LOG_SAMPLE_INTERVAL = 90 // 約 1.5 秒ごとに captureTimeNs を検証用に Log
        private var decodeCount = 0
    }

    override fun initDecode(settings: VideoDecoder.Settings, decodeCallback: VideoDecoder.Callback): VideoCodecStatus {
        return inner.initDecode(settings, decodeCallback)
    }

    override fun decode(
        frame: EncodedImage,
        info: org.webrtc.VideoDecoder.DecodeInfo?
    ): VideoCodecStatus {
        val captureTimeNs = frame.captureTimeNs
        captureTimeStore.pushCaptureTimeNs(captureTimeNs)

        // 検証用: captureTimeNs が非ゼロかを定期的に Log
        if (++decodeCount % LOG_SAMPLE_INTERVAL == 0) {
            Log.d(TAG, "EncodedImage.captureTimeNs = $captureTimeNs (sample #$decodeCount)")
        }

        return inner.decode(frame, info)
    }

    override fun release(): VideoCodecStatus = inner.release()

    override fun getImplementationName(): String {
        val name = try {
            inner.implementationName
        } catch (_: UnsupportedOperationException) {
            "native"
        }
        return "Latency($name)"
    }
}
