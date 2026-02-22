package moe.ryoha.remoterg.ui.viewmodel

import android.app.Application
import android.content.Context
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import moe.ryoha.remoterg.domain.ScreenshotProcessor
import moe.ryoha.remoterg.webrtc.DataChannelMessage
import moe.ryoha.remoterg.webrtc.IWebRtcManager
import moe.ryoha.remoterg.webrtc.WebRtcStats
import moe.ryoha.remoterg.webrtc.signaling.ISignalingClient
import moe.ryoha.remoterg.webrtc.signaling.IncomingMessage
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.webrtc.EglBase
import org.webrtc.IceCandidate
import org.webrtc.VideoTrack

/**
 * ViewerViewModel のユニットテスト
 *
 * IWebRtcManager / ISignalingClient インターフェースにより、
 * Fake 実装でテストが可能。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class ViewerViewModelTest {

    private val testDispatcher = StandardTestDispatcher()

    // Fake WebRtcManager
    private val fakeWebRtcManager = object : IWebRtcManager {
        override val remoteVideoTrack = MutableStateFlow<VideoTrack?>(null)
        override val isConnected = MutableStateFlow(false)
        override val iceConnectionState = MutableStateFlow("NEW")
        override val signalingState = MutableStateFlow("NEW")
        override val rtcStats = MutableStateFlow(WebRtcStats())
        override val dataChannelMessages = MutableSharedFlow<DataChannelMessage>()
        override val eglBaseContext: EglBase.Context get() = mockk(relaxed = true)
        override val localOfferCreated = MutableSharedFlow<String>()
        override val localAnswerCreated = MutableSharedFlow<String>()
        override val iceCandidateCreated = MutableSharedFlow<IceCandidate>()

        var initCalled = false
        var createPeerConnectionCalled = false
        var setupConnectionCalledWith: String? = null
        var closeCalled = false
        var lastVolume: Double? = null

        override fun init(context: Context) { initCalled = true }
        override fun createPeerConnection() { createPeerConnectionCalled = true }
        override fun setupConnection(codec: String) { setupConnectionCalledWith = codec }
        override fun handleRemoteDescription(type: String, sdp: String) {}
        override fun handleIceCandidate(candidate: String, sdpMid: String, sdpMLineIndex: Int) {}
        override fun setAudioVolume(volume: Double) { lastVolume = volume }
        override fun sendDataChannelMessage(message: String) {}
        override fun close() { closeCalled = true }
    }

    // Fake SignalingClient
    private val _fakeMessages = MutableSharedFlow<IncomingMessage>(extraBufferCapacity = 10)
    private val fakeSignalingClient = object : ISignalingClient {
        override val messages: SharedFlow<IncomingMessage> = _fakeMessages
        var connectCalledWith: String? = null
        var disconnectCalled = false
        var lastOfferSdp: String? = null
        var lastOfferCodec: String? = null

        override fun connect(url: String, onConnected: (() -> Unit)?) {
            connectCalledWith = url
            onConnected?.invoke()
        }
        override fun sendOffer(sdp: String, codec: String) {
            lastOfferSdp = sdp
            lastOfferCodec = codec
        }
        override fun sendAnswer(sdp: String) {}
        override fun sendIceCandidate(candidate: String, sdpMid: String, sdpMLineIndex: Int) {}
        override fun disconnect() { disconnectCalled = true }
    }

    private lateinit var viewModel: ViewerViewModel

    @Before
    fun setup() {
        Dispatchers.setMain(testDispatcher)
        val mockApp = mockk<Application>(relaxed = true)
        every { mockApp.applicationContext } returns mockk<Context>(relaxed = true)
        val mockProcessor = mockk<ScreenshotProcessor>(relaxed = true)

        viewModel = ViewerViewModel(
            webRtcManager = fakeWebRtcManager,
            signalingClient = fakeSignalingClient,
            screenshotProcessor = mockProcessor,
            application = mockApp
        )
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `初期状態は Disconnected`() {
        assertEquals("Disconnected", viewModel.connectionState.value)
    }

    @Test
    fun `connectToHost で init, createPeerConnection, connect が呼ばれる`() = runTest {
        viewModel.connectToHost("wss://example.com/ws", "h264")
        advanceUntilIdle()

        assertEquals(true, fakeWebRtcManager.initCalled)
        assertEquals(true, fakeWebRtcManager.createPeerConnectionCalled)
        assertEquals("wss://example.com/ws", fakeSignalingClient.connectCalledWith)
        assertEquals("h264", fakeWebRtcManager.setupConnectionCalledWith)
    }

    @Test
    fun `connectToHost は2度呼んでも1度しか接続しない`() = runTest {
        viewModel.connectToHost("wss://example.com/ws1", "h264")
        advanceUntilIdle()
        viewModel.connectToHost("wss://example.com/ws2", "h264")
        advanceUntilIdle()

        // 最初の URL で接続されているはず
        assertEquals("wss://example.com/ws1", fakeSignalingClient.connectCalledWith)
    }

    @Test
    fun `disconnect で SignalingClient と WebRtcManager が閉じられる`() = runTest {
        viewModel.disconnect()
        advanceUntilIdle()

        assertEquals(true, fakeSignalingClient.disconnectCalled)
        assertEquals(true, fakeWebRtcManager.closeCalled)
        assertEquals("Disconnected", viewModel.connectionState.value)
    }

    @Test
    fun `setAudioVolume が WebRtcManager に委譲される`() {
        viewModel.setAudioVolume(0.5)
        assertEquals(0.5, fakeWebRtcManager.lastVolume)
    }

    @Test
    fun `selectedCodec が connectToHost で設定される`() = runTest {
        viewModel.connectToHost("wss://example.com/ws", "av1")
        advanceUntilIdle()

        assertEquals("av1", viewModel.selectedCodec.value)
    }

    @Test
    fun `Offer SharedFlow 受信時にシグナリングクライアントに送信される`() = runTest {
        viewModel.connectToHost("wss://example.com/ws", "h264")
        advanceUntilIdle()

        // localOfferCreated に SDP を emit
        (fakeWebRtcManager.localOfferCreated as MutableSharedFlow).emit("test_sdp")
        advanceUntilIdle()

        assertEquals("test_sdp", fakeSignalingClient.lastOfferSdp)
        assertEquals("h264", fakeSignalingClient.lastOfferCodec)
    }
}
