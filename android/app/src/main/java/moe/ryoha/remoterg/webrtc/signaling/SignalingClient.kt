package moe.ryoha.remoterg.webrtc.signaling

import android.util.Log
import io.ktor.client.HttpClient
import io.ktor.client.engine.okhttp.OkHttp
import io.ktor.client.plugins.websocket.WebSockets
import io.ktor.client.plugins.websocket.webSocketSession
import io.ktor.websocket.Frame
import io.ktor.websocket.WebSocketSession
import io.ktor.websocket.close
import io.ktor.websocket.readText
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import javax.inject.Inject
import javax.inject.Singleton

/**
 * シグナリングサーバーとの WebSocket 通信を管理するクライアント
 *
 * シグナリングサーバーは session_id と role をクエリパラメータで受け取り、
 * identify メッセージは不要。接続 URL 自体に認証情報が含まれる。
 */
@Singleton
class SignalingClient @Inject constructor() : ISignalingClient {
    private val client = HttpClient(OkHttp) {
        install(WebSockets)
    }

    private var session: WebSocketSession? = null
    private var job: Job? = null
    private val scope = CoroutineScope(Dispatchers.IO)

    // 不明なキーを無視して安全にメッセージをパースする
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true  // type 等のデフォルト値を持つフィールドも必ず出力する
    }

    private val _messages = MutableSharedFlow<IncomingMessage>(
        replay = 10,
        extraBufferCapacity = 10,
        onBufferOverflow = BufferOverflow.DROP_OLDEST
    )
    override val messages: SharedFlow<IncomingMessage> = _messages.asSharedFlow()

    /**
     * シグナリングサーバーに接続する
     * URL には session_id と role がクエリパラメータとして含まれている想定
     * 例: ws://host:port/api/signal?session_id=xxx&role=viewer
     *
     * @param onConnected WebSocket 接続完了時に呼ばれるコールバック。
     *   RN の ws.onopen に相当し、ここで PeerConnection のセットアップを行う。
     */
    override fun connect(url: String, onConnected: (() -> Unit)?) {
        job?.cancel()
        job = scope.launch {
            try {
                session = client.webSocketSession(url)
                Log.d(TAG, "シグナリングサーバーに接続しました")

                // WebSocket 接続完了を通知 — ここで Offer 生成等を開始できる
                onConnected?.invoke()

                for (frame in session!!.incoming) {
                    if (frame is Frame.Text) {
                        val text = frame.readText()
                        Log.d(TAG, "受信: $text")
                        try {
                            val msg = json.decodeFromString<IncomingMessage>(text)
                            _messages.emit(msg)
                        } catch (e: Exception) {
                            Log.e(TAG, "メッセージのパースに失敗", e)
                        }
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "WebSocket エラー", e)
            } finally {
                Log.d(TAG, "シグナリングサーバーから切断されました")
            }
        }
    }

    /** Offer SDP を送信する（指定された codec を付与） */
    override fun sendOffer(sdp: String, codec: String) {
        val msg = OfferMessage(sdp = sdp, codec = codec)
        scope.launch { sendMessage(json.encodeToString(msg)) }
    }

    override fun sendAnswer(sdp: String) {
        val msg = AnswerMessage(sdp = sdp)
        scope.launch { sendMessage(json.encodeToString(msg)) }
    }

    override fun sendIceCandidate(candidate: String, sdpMid: String, sdpMLineIndex: Int) {
        val msg = IceCandidateMessage(
            candidate = candidate,
            sdpMid = sdpMid,
            sdpMLineIndex = sdpMLineIndex
        )
        scope.launch { sendMessage(json.encodeToString(msg)) }
    }

    private suspend fun sendMessage(jsonString: String) {
        if (session?.isActive == true) {
            session?.send(Frame.Text(jsonString))
            Log.d(TAG, "送信: $jsonString")
        } else {
            Log.e(TAG, "セッションが非アクティブのためメッセージを送信できません")
        }
    }

    override fun disconnect() {
        scope.launch {
            session?.close()
            job?.cancel()
        }
    }

    companion object {
        private const val TAG = "SignalingClient"
    }
}
