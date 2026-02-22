package moe.ryoha.remoterg.webrtc

import android.util.Log
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription

// Helper class to reduce boilerplate when observing Sdp events
open class SimpleSdpObserver : SdpObserver {
    override fun onCreateSuccess(p0: SessionDescription?) {}
    override fun onSetSuccess() {}
    override fun onCreateFailure(p0: String?) {
        Log.e("SimpleSdpObserver", "onCreateFailure: $p0")
    }
    override fun onSetFailure(p0: String?) {
        Log.e("SimpleSdpObserver", "onSetFailure: $p0")
    }
}
