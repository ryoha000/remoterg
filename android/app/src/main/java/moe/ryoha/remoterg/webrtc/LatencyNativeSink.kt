package moe.ryoha.remoterg.webrtc

import android.util.Log
import org.webrtc.VideoTrack

/**
 * JNI 経由で C++ LatencyVideoSink を VideoTrack にアタッチし、
 * フレーム単位で packet_infos.absolute_capture_time (Unix ms) を取得する。
 */
class LatencyNativeSink {

    fun interface Callback {
        fun onCaptureTime(status: Int, captureUnixMs: Long, timestampUs: Long)
    }

    private var nativeSinkPtr: Long = 0
    private var nativeTrackPtr: Long = 0
    private var disabled = false
    private var disableReason: String? = null

    @Synchronized
    fun attachToTrack(track: VideoTrack, callback: Callback): Boolean {
        if (disabled) {
            Log.w(TAG, "attachToTrack skipped: native sink is disabled reason=$disableReason")
            return false
        }

        val nextTrackPtr = getNativeTrackPtr(track)
        if (nextTrackPtr == 0L) {
            disableLocked("native video track pointer is 0")
            return false
        }

        if (nativeSinkPtr == 0L) {
            nativeSinkPtr = try {
                nativeCreateLatencySink(callback)
            } catch (t: Throwable) {
                Log.e(TAG, "nativeCreateLatencySink threw", t)
                0L
            }
            if (nativeSinkPtr == 0L) {
                disableLocked("nativeCreateLatencySink failed")
                return false
            }
        }

        if (nativeTrackPtr == nextTrackPtr) {
            return true
        }

        if (nativeTrackPtr != 0L) {
            try {
                nativeDetachFromTrack(nativeTrackPtr, nativeSinkPtr)
            } catch (t: Throwable) {
                disableLocked("nativeDetachFromTrack failed", t)
                return false
            }
            nativeTrackPtr = 0L
        }

        try {
            nativeAttachToTrack(nextTrackPtr, nativeSinkPtr)
        } catch (t: Throwable) {
            disableLocked("nativeAttachToTrack failed", t)
            return false
        }
        nativeTrackPtr = nextTrackPtr
        return true
    }

    @Synchronized
    fun detach() {
        if (nativeSinkPtr != 0L && nativeTrackPtr != 0L) {
            try {
                nativeDetachFromTrack(nativeTrackPtr, nativeSinkPtr)
            } catch (t: Throwable) {
                Log.e(TAG, "nativeDetachFromTrack threw", t)
            }
        }
        nativeTrackPtr = 0L
    }

    @Synchronized
    fun release() {
        detach()
        if (nativeSinkPtr != 0L) {
            try {
                nativeDestroySink(nativeSinkPtr)
            } catch (t: Throwable) {
                Log.e(TAG, "nativeDestroySink threw", t)
            }
        }
        nativeSinkPtr = 0L
    }

    fun isEnabled(): Boolean = !disabled

    private fun getNativeTrackPtr(track: VideoTrack): Long {
        return try {
            track.nativeVideoTrack
        } catch (e: Throwable) {
            Log.e(TAG, "Failed to get native track ptr", e)
            0L
        }
    }

    private fun disableLocked(reason: String, throwable: Throwable? = null) {
        if (disabled) return
        disabled = true
        disableReason = reason
        if (throwable != null) {
            Log.e(TAG, "Disabling native sink: $reason", throwable)
        } else {
            Log.e(TAG, "Disabling native sink: $reason")
        }

        val sinkPtr = nativeSinkPtr
        val trackPtr = nativeTrackPtr
        nativeTrackPtr = 0L
        nativeSinkPtr = 0L

        if (sinkPtr != 0L && trackPtr != 0L) {
            try {
                nativeDetachFromTrack(trackPtr, sinkPtr)
            } catch (t: Throwable) {
                Log.e(TAG, "nativeDetachFromTrack during disable threw", t)
            }
        }
        if (sinkPtr != 0L) {
            try {
                nativeDestroySink(sinkPtr)
            } catch (t: Throwable) {
                Log.e(TAG, "nativeDestroySink during disable threw", t)
            }
        }
    }

    companion object {
        private const val TAG = "LatencyNativeSink"

        init {
            try {
                System.loadLibrary("latency_sink")
            } catch (e: UnsatisfiedLinkError) {
                Log.e(TAG, "Failed to load latency_sink", e)
            }
        }

        @JvmStatic
        private external fun nativeCreateLatencySink(callback: Callback): Long

        @JvmStatic
        private external fun nativeAttachToTrack(nativeTrack: Long, nativeSink: Long)

        @JvmStatic
        private external fun nativeDetachFromTrack(nativeTrack: Long, nativeSink: Long)

        @JvmStatic
        private external fun nativeDestroySink(nativeSink: Long)
    }
}
