package moe.ryoha.remoterg.domain

import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive
import moe.ryoha.remoterg.data.repository.ScreenshotRepository
import moe.ryoha.remoterg.webrtc.DataChannelMessage
import moe.ryoha.remoterg.webrtc.WebRtcManager
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class ScreenshotProcessor @Inject constructor(
    private val webRtcManager: WebRtcManager,
    private val repository: ScreenshotRepository
) {
    private var observeJob: Job? = null
    private val scope = CoroutineScope(Dispatchers.IO)

    // Current assembling screenshot state
    private var currentId: String? = null
    private var currentSize: Int = 0
    private var currentFormat: String = "png"
    private var currentReceived: Int = 0
    private var currentChunks = mutableListOf<ByteArray>()
    
    // Metadata
    private var windowTitle: String? = null
    private var processPath: String? = null
    private var processName: String? = null

    // For notifying UI
    private val _onScreenshotSaved = MutableSharedFlow<android.net.Uri>()
    val onScreenshotSaved = _onScreenshotSaved.asSharedFlow()

    fun startObserving() {
        if (observeJob != null) return
        observeJob = scope.launch {
            webRtcManager.dataChannelMessages.collect { msg ->
                handleMessage(msg)
            }
        }
    }

    fun stopObserving() {
        observeJob?.cancel()
        observeJob = null
    }

    private suspend fun handleMessage(msg: DataChannelMessage) {
        when (msg) {
            is DataChannelMessage.Text -> {
                // Parse JSON
                try {
                    val root = Json.parseToJsonElement(msg.text)
                    if (root is JsonObject) {
                        // Check if it's a Screenshot (metadata from host)
                        val screenshotMetadata = root["SCREENSHOT_METADATA"]?.jsonObject?.get("payload")?.jsonObject
                        if (screenshotMetadata != null && screenshotMetadata.containsKey("id") && screenshotMetadata.containsKey("size")) {
                            Log.d(TAG, "Screenshot metadata received: ${msg.text}")
                            currentId = screenshotMetadata["id"]?.jsonPrimitive?.content
                            currentSize = screenshotMetadata["size"]?.jsonPrimitive?.intOrNull ?: 0
                            currentFormat = screenshotMetadata["format"]?.jsonPrimitive?.content ?: "png"
                            windowTitle = screenshotMetadata["window_title"]?.jsonPrimitive?.content
                            processPath = screenshotMetadata["process_path"]?.jsonPrimitive?.content
                            processName = screenshotMetadata["process_name"]?.jsonPrimitive?.content
                            
                            currentReceived = 0
                            currentChunks.clear()
                        }
                    }
                } catch (e: Exception) {
                    Log.e(TAG, "Error parsing JSON text message", e)
                }
            }
            is DataChannelMessage.Binary -> {
                if (currentId != null) {
                    currentChunks.add(msg.data)
                    currentReceived += msg.data.size
                    
                    if (currentReceived >= currentSize) {
                        Log.d(TAG, "Screenshot all chunks received. Total: $currentReceived bytes")
                        processFinishedScreenshot()
                    }
                }
            }
        }
    }

    private suspend fun processFinishedScreenshot() {
        val id = currentId ?: return
        val size = currentSize
        val format = currentFormat
        
        // Assemble chunks
        val combined = ByteArray(size)
        var offset = 0
        for (chunk in currentChunks) {
            val length = minOf(chunk.size, size - offset)
            System.arraycopy(chunk, 0, combined, offset, length)
            offset += length
        }

        // Save
        val uri = repository.saveScreenshot(
            hostId = id,
            format = format,
            data = combined,
            windowTitle = windowTitle,
            processPath = processPath,
            processName = processName
        )

        // Reset state so it doesn't get saved again on subsequent messages
        currentId = null
        currentSize = 0
        currentReceived = 0
        currentChunks.clear()

        if (uri != null) {
            _onScreenshotSaved.emit(uri)
        }
    }

    /**
     * DataChannel に Screenshot リクエストを送信する
     */
    fun requestScreenshot() {
        Log.d(TAG, "Sending ScreenshotRequest")
        val req = "{\"ScreenshotRequest\":null}"
        webRtcManager.sendDataChannelMessage(req)
    }

    companion object {
        private const val TAG = "ScreenshotProcessor"
    }
}
