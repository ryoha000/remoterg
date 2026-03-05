package moe.ryoha.remoterg.webrtc

import android.util.Log
import org.webrtc.MediaStreamTrack
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

    fun attachToTrack(track: VideoTrack, callback: Callback) {
        val nextTrackPtr = getNativeTrackPtr(track)
        if (nextTrackPtr == 0L) return

        if (nativeSinkPtr == 0L) {
            nativeSinkPtr = nativeCreateLatencySink(callback)
            if (nativeSinkPtr == 0L) {
                Log.e(TAG, "nativeCreateLatencySink failed")
                return
            }
        }

        if (nativeTrackPtr == nextTrackPtr) {
            return
        }

        if (nativeTrackPtr != 0L) {
            nativeDetachFromTrack(nativeTrackPtr, nativeSinkPtr)
            nativeTrackPtr = 0L
        }

        nativeAttachToTrack(nextTrackPtr, nativeSinkPtr)
        nativeTrackPtr = nextTrackPtr
    }

    fun detach() {
        if (nativeSinkPtr != 0L && nativeTrackPtr != 0L) {
            nativeDetachFromTrack(nativeTrackPtr, nativeSinkPtr)
        }
        nativeTrackPtr = 0L
    }

    fun release() {
        detach()
        if (nativeSinkPtr != 0L) {
            nativeDestroySink(nativeSinkPtr)
        }
        nativeSinkPtr = 0L
    }

    private fun getNativeTrackPtr(track: VideoTrack): Long {
        return try {
            val method = MediaStreamTrack::class.java.getDeclaredMethod("getNativeMediaStreamTrack")
            method.isAccessible = true
            method.invoke(track) as? Long ?: 0L
        } catch (e: Exception) {
            Log.e(TAG, "Failed to get native track ptr", e)
            0L
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
