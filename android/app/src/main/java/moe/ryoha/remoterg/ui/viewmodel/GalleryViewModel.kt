package moe.ryoha.remoterg.ui.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import moe.ryoha.remoterg.data.local.dao.ScreenshotDao
import moe.ryoha.remoterg.data.local.dao.AnalysisDao
import moe.ryoha.remoterg.data.local.entity.AnalysisResultEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotFavoriteEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotMapEntity
import moe.ryoha.remoterg.data.repository.ScreenshotRepository
import moe.ryoha.remoterg.data.repository.MediaStoreScreenshot
import moe.ryoha.remoterg.webrtc.IWebRtcManager
import moe.ryoha.remoterg.data.model.AnalysisResult
import org.json.JSONObject
import kotlinx.serialization.json.Json
import kotlinx.serialization.decodeFromString
import android.util.Log
import javax.inject.Inject

data class SearchFilters(
    val text: String = "",
    val since: Long? = null,
    val until: Long? = null,
    val gameTitle: String? = null,
    val isFavorite: Boolean = false
)

data class JustifiedItem(
    val screenshot: MediaStoreScreenshot,
    val isFavorite: Boolean,
    val aspectRatio: Float,
    val widthDp: Float,
)

data class JustifiedRow(
    val items: List<JustifiedItem>,
    val isLastRow: Boolean,
    val rowHeightDp: Float,
)

data class DateSection(
    val title: String,
    val rows: List<JustifiedRow>
)

@HiltViewModel
class GalleryViewModel @Inject constructor(
    private val repository: ScreenshotRepository,
    private val screenshotDao: ScreenshotDao,
    private val analysisDao: AnalysisDao,
    private val webRtcManager: IWebRtcManager
) : ViewModel() {

    private val _searchFilters = MutableStateFlow(SearchFilters())
    val searchFilters: StateFlow<SearchFilters> = _searchFilters.asStateFlow()

    // スクリーンショットの読み込み完了フラグ（初期値 false → 最初のデータ emit 後 true）
    private val _isScreenshotsLoaded = MutableStateFlow(false)
    val isScreenshotsLoaded: StateFlow<Boolean> = _isScreenshotsLoaded.asStateFlow()

    val favorites = screenshotDao.getAllFavorites()
        .stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5000),
            initialValue = emptyList()
        )

    val screenshots = combine(
        repository.getAllScreenshotsWithDimensions(),
        favorites,
        _searchFilters
    ) { allScreenshots, favs, filters ->
        // 最初のデータが到着したら読み込み完了とする
        if (!_isScreenshotsLoaded.value) {
            _isScreenshotsLoaded.value = true
        }
        val favIds = favs.map { it.localId }.toSet()
        allScreenshots.filter { item ->
            // Date filters
            if (filters.since != null && item.dateAdded * 1000 < filters.since) return@filter false
            if (filters.until != null && item.dateAdded * 1000 > filters.until) return@filter false
            
            // Favorite filter
            if (filters.isFavorite && !favIds.contains(item.localId)) return@filter false

            // Game title filter
            if (filters.gameTitle != null && item.windowTitle != filters.gameTitle) return@filter false

            // Text search (matches title or process name)
            if (filters.text.isNotBlank()) {
                val query = filters.text.lowercase()
                val matchTitle = item.windowTitle.lowercase().contains(query)
                val matchProcess = item.processName.lowercase().contains(query)
                if (!matchTitle && !matchProcess) return@filter false
            }
            
            true
        }
    }.stateIn(
        scope = viewModelScope,
        started = SharingStarted.WhileSubscribed(5000),
        initialValue = emptyList()
    )

    // Extract unique titles for the title cards UI
    val recentTitles = repository.getAllScreenshotsWithDimensions()
        .map { list ->
            // Group by windowTitle and get the most recent one for the thumbnail
            list.filter { it.windowTitle.isNotBlank() }
                .groupBy { it.windowTitle }
                .map { (title, items) -> 
                    // items are sorted descending by dateAdded from repository
                    Pair(title, items.first())
                }
                .take(10) // Limit to top 10 recent titles
        }
        .stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5000),
            initialValue = emptyList()
        )

    private val _screenWidthDp = MutableStateFlow(0f)
    fun updateScreenWidth(widthDp: Float) {
        if (_screenWidthDp.value != widthDp) {
            _screenWidthDp.value = widthDp
        }
    }

    // --- AI Analysis ---
    private val analysisBuffers = mutableMapOf<String, java.lang.StringBuilder>()
    private val jsonParser = Json { ignoreUnknownKeys = true }

    private val _analysisResults = MutableStateFlow<Map<String, AnalysisResult>>(emptyMap())
    val analysisResults: StateFlow<Map<String, AnalysisResult>> = _analysisResults.asStateFlow()

    private val _isAnalyzingMap = MutableStateFlow<Map<String, Boolean>>(emptyMap())
    val isAnalyzingMap: StateFlow<Map<String, Boolean>> = _isAnalyzingMap.asStateFlow()

    val isConnected: StateFlow<Boolean> = webRtcManager.isConnected

    init {
        viewModelScope.launch {
            webRtcManager.dataChannelMessages.collect { msg ->
                if (msg is moe.ryoha.remoterg.webrtc.DataChannelMessage.Text) {
                    handleDataChannelMessage(msg.text)
                }
            }
        }
    }

    private suspend fun handleDataChannelMessage(text: String) {
        try {
            val jsonObject = JSONObject(text)
            when {
                jsonObject.has("ANALYZE_RESPONSE") -> {
                    val resp = jsonObject.getJSONObject("ANALYZE_RESPONSE")
                    val id = resp.getString("id")
                    val resultText = resp.getString("text")
                    saveAndEmitAnalysis(id, resultText, false)
                }
                jsonObject.has("ANALYZE_RESPONSE_CHUNK") -> {
                    val resp = jsonObject.getJSONObject("ANALYZE_RESPONSE_CHUNK")
                    val id = resp.getString("id")
                    val delta = resp.getString("delta")
                    
                    val buffer = analysisBuffers.getOrPut(id) { java.lang.StringBuilder() }
                    buffer.append(delta)
                    
                    _isAnalyzingMap.value = _isAnalyzingMap.value.toMutableMap().apply { put(id, true) }
                }
                jsonObject.has("ANALYZE_RESPONSE_DONE") -> {
                    val resp = jsonObject.getJSONObject("ANALYZE_RESPONSE_DONE")
                    val id = resp.getString("id")
                    val buffer = analysisBuffers.remove(id)
                    if (buffer != null) {
                        saveAndEmitAnalysis(id, buffer.toString(), false)
                    }
                }
            }
        } catch (e: Exception) {
            // Log.e("GalleryViewModel", "JSON Parse error for Analysis", e)
        }
    }

    private suspend fun saveAndEmitAnalysis(hostId: String, jsonString: String, isPartial: Boolean) {
        try {
            val resultObj = jsonParser.decodeFromString<AnalysisResult>(jsonString)
            
            val localScreenshots = screenshotDao.getScreenshotsByHostId(hostId)
            localScreenshots.forEach { ss ->
                analysisDao.insertAnalysisResult(
                    AnalysisResultEntity(
                        localId = ss.localId, 
                        data = jsonString, 
                        createdAt = System.currentTimeMillis()
                    )
                )
                _analysisResults.value = _analysisResults.value.toMutableMap().apply {
                    put(ss.localId, resultObj)
                }
            }
            if (!isPartial) {
                _isAnalyzingMap.value = _isAnalyzingMap.value.toMutableMap().apply { put(hostId, false) }
            }
        } catch (e: Exception) {
            Log.e("GalleryViewModel", "Failed to deserialize analysis result", e)
        }
    }

    fun loadAnalysisResult(localId: String) {
        viewModelScope.launch {
            if (!_analysisResults.value.containsKey(localId)) {
                val entity = analysisDao.getAnalysisResult(localId)
                if (entity != null) {
                    try {
                        val resultObj = jsonParser.decodeFromString<AnalysisResult>(entity.data)
                        _analysisResults.value = _analysisResults.value.toMutableMap().apply {
                            put(localId, resultObj)
                        }
                    } catch (e: Exception) {
                        Log.e("GalleryViewModel", "Failed to decode DB analysis result", e)
                    }
                }
            }
        }
    }

    fun requestAnalyze(hostId: String, maxEdge: Int = 512) {
        if (!webRtcManager.isConnected.value) return
        _isAnalyzingMap.value = _isAnalyzingMap.value.toMutableMap().apply { put(hostId, true) }
        
        val msg = JSONObject().apply {
            put("AnalyzeRequest", JSONObject().apply {
                put("id", hostId)
                put("max_edge", maxEdge)
            })
        }.toString()
        webRtcManager.sendDataChannelMessage(msg)
    }

    // -------------------

    val sections = combine(screenshots, _screenWidthDp, favorites) { screenshotList, screenWidthDp, favList ->
        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) {
            val favIds = favList.map { it.localId }.toSet()
            moe.ryoha.remoterg.ui.util.JustifiedLayoutCalculator.calculateSections(
                screenshots = screenshotList,
                favoriteIds = favIds,
                screenWidthDp = screenWidthDp
            )
        }
    }.stateIn(
        scope = viewModelScope,
        started = SharingStarted.WhileSubscribed(5000),
        initialValue = emptyList()
    )


    fun updateFilters(newFilters: SearchFilters) {
        _searchFilters.value = newFilters
    }

    fun toggleFavorite(localId: String, isFavorite: Boolean) {
        viewModelScope.launch {
            if (isFavorite) {
                screenshotDao.deleteFavorite(localId)
            } else {
                screenshotDao.insertFavorite(ScreenshotFavoriteEntity(localId = localId))
            }
        }
    }

    fun deleteScreenshot(localId: String) {
        viewModelScope.launch {
            repository.deleteScreenshot(localId)
        }
    }
}
