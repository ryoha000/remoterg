package moe.ryoha.remoterg.webrtc

import java.util.ArrayDeque

class FrameNativeMatchStore(
    private val ttlMs: Long = 1_000L
) {
    data class MatchedSample(
        val captureUnixMs: Long,
        val timestampUs: Long,
        val tCapHostdMonoMs: Double,
        val framePendingAfterMatch: Int,
        val nativePendingAfterMatch: Int
    )

    private data class FrameSample(
        val tCapHostdMonoMs: Double,
        val receivedElapsedMs: Long
    )

    private data class NativeSample(
        val timestampUs: Long,
        val receivedElapsedMs: Long
    )

    private val frameByCaptureUnix = HashMap<Long, ArrayDeque<FrameSample>>()
    private val nativeByCaptureUnix = HashMap<Long, ArrayDeque<NativeSample>>()

    @Synchronized
    fun offerFrame(
        captureUnixMs: Long,
        tCapHostdMonoMs: Double,
        receivedElapsedMs: Long
    ): MatchedSample? {
        trimLocked(receivedElapsedMs)
        val nativeQueue = nativeByCaptureUnix[captureUnixMs]
        if (nativeQueue != null && nativeQueue.isNotEmpty()) {
            val native = nativeQueue.removeFirst()
            if (nativeQueue.isEmpty()) {
                nativeByCaptureUnix.remove(captureUnixMs)
            }
            return MatchedSample(
                captureUnixMs = captureUnixMs,
                timestampUs = native.timestampUs,
                tCapHostdMonoMs = tCapHostdMonoMs,
                framePendingAfterMatch = pendingFramesLocked(),
                nativePendingAfterMatch = pendingNativesLocked()
            )
        }

        val frameQueue = frameByCaptureUnix.getOrPut(captureUnixMs) { ArrayDeque() }
        frameQueue.addLast(FrameSample(tCapHostdMonoMs = tCapHostdMonoMs, receivedElapsedMs = receivedElapsedMs))
        return null
    }

    @Synchronized
    fun offerNative(
        captureUnixMs: Long,
        timestampUs: Long,
        receivedElapsedMs: Long
    ): MatchedSample? {
        trimLocked(receivedElapsedMs)
        val frameQueue = frameByCaptureUnix[captureUnixMs]
        if (frameQueue != null && frameQueue.isNotEmpty()) {
            val frame = frameQueue.removeFirst()
            if (frameQueue.isEmpty()) {
                frameByCaptureUnix.remove(captureUnixMs)
            }
            return MatchedSample(
                captureUnixMs = captureUnixMs,
                timestampUs = timestampUs,
                tCapHostdMonoMs = frame.tCapHostdMonoMs,
                framePendingAfterMatch = pendingFramesLocked(),
                nativePendingAfterMatch = pendingNativesLocked()
            )
        }

        val nativeQueue = nativeByCaptureUnix.getOrPut(captureUnixMs) { ArrayDeque() }
        nativeQueue.addLast(NativeSample(timestampUs = timestampUs, receivedElapsedMs = receivedElapsedMs))
        return null
    }

    @Synchronized
    fun pendingFrameCount(): Int = pendingFramesLocked()

    @Synchronized
    fun pendingNativeCount(): Int = pendingNativesLocked()

    @Synchronized
    fun clear() {
        frameByCaptureUnix.clear()
        nativeByCaptureUnix.clear()
    }

    @Synchronized
    private fun trimLocked(nowElapsedMs: Long) {
        trimMapLocked(frameByCaptureUnix) { nowElapsedMs - it.receivedElapsedMs > ttlMs }
        trimMapLocked(nativeByCaptureUnix) { nowElapsedMs - it.receivedElapsedMs > ttlMs }
    }

    private fun <T> trimMapLocked(
        map: MutableMap<Long, ArrayDeque<T>>,
        isExpired: (T) -> Boolean
    ) {
        val iterator = map.entries.iterator()
        while (iterator.hasNext()) {
            val entry = iterator.next()
            val queue = entry.value
            while (queue.isNotEmpty() && isExpired(queue.first())) {
                queue.removeFirst()
            }
            if (queue.isEmpty()) {
                iterator.remove()
            }
        }
    }

    private fun pendingFramesLocked(): Int = frameByCaptureUnix.values.sumOf { it.size }

    private fun pendingNativesLocked(): Int = nativeByCaptureUnix.values.sumOf { it.size }
}
