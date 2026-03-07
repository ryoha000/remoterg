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
import moe.ryoha.remoterg.data.repository.GoogleDriveRepository
import moe.ryoha.remoterg.webrtc.DataChannelMessage
import moe.ryoha.remoterg.webrtc.IWebRtcManager
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class ScreenshotProcessor @Inject constructor(
    private val webRtcManager: IWebRtcManager,
    private val repository: ScreenshotRepository,
    private val analysisDao: moe.ryoha.remoterg.data.local.dao.AnalysisDao,
    private val screenshotDao: moe.ryoha.remoterg.data.local.dao.ScreenshotDao,
    private val gameDao: moe.ryoha.remoterg.data.local.dao.GameDao,
    private val googleDriveRepository: GoogleDriveRepository
) {
    private var observeJob: Job? = null
    private val scope = CoroutineScope(Dispatchers.IO)

    // Current assembling screenshot state
    private var currentId: String? = null
    private var currentSize: Int = 0
    private var currentFormat: String = "png"
    private var currentReceived: Int = 0
    private var currentChunks = mutableListOf<ByteArray>()
    
    // Auto AI Analysis Buffers
    private val analysisBuffers = mutableMapOf<String, StringBuilder>()

    // Pending local bitmap to be saved when metadata arrives
    var pendingLocalBitmap: android.graphics.Bitmap? = null
    
    // Metadata
    private var windowTitle: String? = null
    private var processPath: String? = null
    private var processName: String? = null
    private var vndbId: String? = null
    private var officialTitle: String? = null

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
                            vndbId = screenshotMetadata["vndb_id"]?.jsonPrimitive?.content
                            officialTitle = screenshotMetadata["official_title"]?.jsonPrimitive?.content
                            
                            currentReceived = 0
                            currentChunks.clear()

                            if (currentSize == 0) {
                                Log.d(TAG, "Screenshot metadata size is 0, processing finished immediately using local bitmap")
                                processFinishedScreenshot()
                            }
                        } else if (root.containsKey("ANALYZE_RESPONSE") || root.containsKey("ANALYZE_RESPONSE_CHUNK") || root.containsKey("ANALYZE_RESPONSE_DONE")) {
                            handleAnalysisMessage(msg.text)
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

    private suspend fun handleAnalysisMessage(text: String) {
        try {
            val jsonObject = org.json.JSONObject(text)
            when {
                jsonObject.has("ANALYZE_RESPONSE") -> {
                    val resp = jsonObject.getJSONObject("ANALYZE_RESPONSE")
                    val id = resp.getString("id")
                    val resultText = resp.getString("text")
                    saveAnalysisToDb(id, resultText)
                }
                jsonObject.has("ANALYZE_RESPONSE_CHUNK") -> {
                    val resp = jsonObject.getJSONObject("ANALYZE_RESPONSE_CHUNK")
                    val id = resp.getString("id")
                    val delta = resp.getString("delta")
                    
                    val buffer = analysisBuffers.getOrPut(id) { StringBuilder() }
                    buffer.append(delta)
                }
                jsonObject.has("ANALYZE_RESPONSE_DONE") -> {
                    val resp = jsonObject.getJSONObject("ANALYZE_RESPONSE_DONE")
                    val id = resp.getString("id")
                    val buffer = analysisBuffers.remove(id)
                    if (buffer != null) {
                        saveAnalysisToDb(id, buffer.toString())
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error handling analysis message", e)
        }
    }

    private suspend fun saveAnalysisToDb(hostId: String, jsonString: String) {
        try {
            val jsonParser = Json { ignoreUnknownKeys = true }
            val result = try {
                jsonParser.decodeFromString<moe.ryoha.remoterg.data.model.AnalysisResult>(jsonString)
            } catch (e: Exception) {
                Log.e(TAG, "Failed to parse analysis result JSON: $e")
                null
            }

            val localScreenshots = screenshotDao.getScreenshotsByHostId(hostId)
            localScreenshots.forEach { ss ->
                analysisDao.insertAnalysisResult(
                    moe.ryoha.remoterg.data.local.entity.AnalysisResultEntity(
                        localId = ss.localId, 
                        data = jsonString, 
                        createdAt = System.currentTimeMillis()
                    )
                )

                if (result != null) {
                    result.sceneInfo?.let { scene ->
                        analysisDao.insertAnalysisScene(
                            moe.ryoha.remoterg.data.local.entity.AnalysisSceneEntity(
                                localId = ss.localId,
                                location = scene.location,
                                timeOfDay = scene.timeOfDay,
                                atmosphere = scene.atmosphere
                            )
                        )
                    }

                    result.dialogue?.let { dialogue ->
                        analysisDao.insertAnalysisDialogue(
                            moe.ryoha.remoterg.data.local.entity.AnalysisDialogueEntity(
                                localId = ss.localId,
                                speaker = dialogue.speaker,
                                text = dialogue.text
                            )
                        )
                    }

                    if (result.characters.isNotEmpty()) {
                        val entities = result.characters.map { char ->
                            moe.ryoha.remoterg.data.local.entity.AnalysisCharacterEntity(
                                localId = ss.localId,
                                name = char.name,
                                expressionTags = char.expressionTags.joinToString(","),
                                visualDescription = char.visualDescription,
                                position = char.position
                            )
                        }
                        analysisDao.insertAnalysisCharacters(entities)
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to save analysis result to DB: $e")
        }
    }

    private suspend fun processFinishedScreenshot() {
        val id = currentId ?: return
        val size = currentSize
        val format = currentFormat

        val gameId = vndbId?.let { vId ->
            gameDao.upsertGame(vId, officialTitle)
        }
        
        val uri = if (size == 0) {
            val bitmap = pendingLocalBitmap
            if (bitmap != null) {
                val retUri = repository.saveLocalScreenshot(
                    bitmap = bitmap,
                    hostId = id,
                    windowTitle = windowTitle,
                    processPath = processPath,
                    processName = processName,
                    gameId = gameId
                )
                
                if (googleDriveRepository.isConnected.value) {
                    val stream = java.io.ByteArrayOutputStream()
                    bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 90, stream)
                    val byteArray = stream.toByteArray()
                    scope.launch {
                        googleDriveRepository.uploadToDrive(byteArray, "jpeg", windowTitle, processName)
                    }
                }
                
                retUri
            } else {
                Log.e(TAG, "Local screenshot bitmap is missing when receiving size=0 metadata")
                null
            }
        } else {
            // Assemble chunks
            val combined = ByteArray(size)
            var offset = 0
            for (chunk in currentChunks) {
                val length = minOf(chunk.size, size - offset)
                System.arraycopy(chunk, 0, combined, offset, length)
                offset += length
            }

            // Save
            val retUri = repository.saveScreenshot(
                hostId = id,
                format = format,
                data = combined,
                windowTitle = windowTitle,
                processPath = processPath,
                processName = processName,
                gameId = gameId
            )
            
            if (googleDriveRepository.isConnected.value) {
                scope.launch {
                    googleDriveRepository.uploadToDrive(combined, format, windowTitle, processName)
                }
            }
            
            retUri
        }

        // Reset state so it doesn't get saved again on subsequent messages
        currentId = null
        currentSize = 0
        currentReceived = 0
        currentChunks.clear()
        pendingLocalBitmap = null
        vndbId = null
        officialTitle = null

        if (uri != null) {
            _onScreenshotSaved.emit(uri)
        }
    }

    /**
     * DataChannel に Screenshot リクエストを送信する
     */
    fun requestScreenshot(includeImage: Boolean) {
        Log.d(TAG, "Sending ScreenshotRequest (includeImage=$includeImage)")
        val req = "{\"ScreenshotRequest\":{\"include_image\":$includeImage}}"
        webRtcManager.sendDataChannelMessage(req)
    }

    /**
     * クライアント側でキャプチャしたスクリーンショットを保存する
     */
    suspend fun saveLocalScreenshot(bitmap: android.graphics.Bitmap) {
        val uri = repository.saveLocalScreenshot(bitmap)
        if (uri != null) {
            _onScreenshotSaved.emit(uri)
        }
        
        if (googleDriveRepository.isConnected.value) {
            val stream = java.io.ByteArrayOutputStream()
            bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 90, stream)
            val byteArray = stream.toByteArray()
            scope.launch {
                googleDriveRepository.uploadToDrive(byteArray, "jpeg", "Remoterg Local Capture", "moe.ryoha.remoterg")
            }
        }
    }

    companion object {
        private const val TAG = "ScreenshotProcessor"
    }
}
