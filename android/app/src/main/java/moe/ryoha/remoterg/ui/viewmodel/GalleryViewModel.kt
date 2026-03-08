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
    val charaText: String? = null,
    val dialogText: String? = null,
    val speakerText: String? = null,
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
    private val webRtcManager: IWebRtcManager,
    private val settingsRepository: moe.ryoha.remoterg.data.repository.SettingsRepository
) : ViewModel() {

    private val _searchFilters = MutableStateFlow(SearchFilters())
    val searchFilters: StateFlow<SearchFilters> = _searchFilters.asStateFlow()

    private val _charaMatchedIds = MutableStateFlow<Set<String>?>(null)
    private val _textMatchedIds = MutableStateFlow<Set<String>?>(null)
    private val _speakerMatchedIds = MutableStateFlow<Set<String>?>(null)

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
        _searchFilters,
        _charaMatchedIds,
        _textMatchedIds,
        _speakerMatchedIds
    ) { params ->
        val allScreenshots = params[0] as List<MediaStoreScreenshot>
        val favs = params[1] as List<ScreenshotFavoriteEntity>
        val filters = params[2] as SearchFilters
        val charaMatchedIds = params[3] as Set<String>?
        val textMatchedIds = params[4] as Set<String>?
        val speakerMatchedIds = params[5] as Set<String>?

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
            // 検索時の gameTitle は直値か windowTitle の prefix である可能性がある
            if (filters.gameTitle != null) {
                val matchGameTitle = item.gameTitle == filters.gameTitle
                val matchWindowTitle = item.windowTitle.startsWith(filters.gameTitle)
                if (!matchGameTitle && !matchWindowTitle) return@filter false
            }

            // Character text filter
            if (filters.charaText != null && charaMatchedIds?.contains(item.localId) != true) return@filter false

            // Speaker text filter
            if (filters.speakerText != null && speakerMatchedIds?.contains(item.localId) != true) return@filter false

            // Dialogue text filter
            if (filters.dialogText != null && textMatchedIds?.contains(item.localId) != true) return@filter false

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
            // gameTitle があるものは gameTitle、次に processPath、最後に windowTitle でグループ化する
            val grouped = list.filter { it.windowTitle.isNotBlank() || !it.gameTitle.isNullOrBlank() }
                .groupBy { it.gameTitle?.takeIf { gt -> gt.isNotBlank() }
                    ?: it.processPath?.takeIf { p -> p.isNotBlank() } 
                    ?: it.windowTitle }

            grouped.map { (_, items) ->
                // items are sorted descending by dateAdded from repository
                val latestScreenshot = items.first()

                // gameTitle があればそれを表示タイトルとし、なければ最新の windowTitle を使う
                val displayTitle = latestScreenshot.gameTitle?.takeIf { it.isNotBlank() } 
                    ?: latestScreenshot.windowTitle

                Pair(displayTitle, latestScreenshot)
            }
        }
        .stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5000),
            initialValue = emptyList()
        )

    val recentCharacters = analysisDao.getRecentCharacters()
        .stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5000),
            initialValue = emptyList()
        )

    val recentSpeakers = analysisDao.getRecentSpeakers()
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
                    
                    val charactersList = mutableListOf<moe.ryoha.remoterg.data.model.Character>()
                    if (resp.has("characters")) {
                        val charsArray = resp.getJSONArray("characters")
                        for (i in 0 until charsArray.length()) {
                            val charObj = charsArray.getJSONObject(i)
                            charactersList.add(
                                moe.ryoha.remoterg.data.model.Character(
                                    name = charObj.optString("name", ""),
                                    position = charObj.optString("position", ""),
                                    expressionTags = emptyList(),
                                    visualDescription = ""
                                )
                            )
                        }
                    }
                    saveAndEmitAnalysis(id, resultText, false, charactersList)
                }
            }
        } catch (e: Exception) {
            // Log.e("GalleryViewModel", "JSON Parse error for Analysis", e)
        }
    }

    private suspend fun saveAndEmitAnalysis(hostId: String, jsonString: String, isPartial: Boolean, characters: List<moe.ryoha.remoterg.data.model.Character> = emptyList()) {
        try {
            val resultObj = jsonParser.decodeFromString<AnalysisResult>(jsonString).copy(characters = characters)
            
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
                        
                        val characters = analysisDao.getAnalysisCharacters(localId).map {
                            moe.ryoha.remoterg.data.model.Character(
                                name = it.name,
                                position = it.position,
                                expressionTags = emptyList(),
                                visualDescription = ""
                            )
                        }
                        
                        _analysisResults.value = _analysisResults.value.toMutableMap().apply {
                            put(localId, resultObj.copy(characters = characters))
                        }
                    } catch (e: Exception) {
                        Log.e("GalleryViewModel", "Failed to decode DB analysis result", e)
                    }
                }
            }
        }
    }

    fun requestAnalyze(hostId: String) {
        if (!webRtcManager.isConnected.value) return
        _isAnalyzingMap.value = _isAnalyzingMap.value.toMutableMap().apply { put(hostId, true) }
        
        val maxEdge = settingsRepository.maxEdge.value
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
        
        viewModelScope.launch {
            if (newFilters.charaText != null) {
                _charaMatchedIds.value = analysisDao.searchLocalIdsByCharacter(newFilters.charaText).toSet()
            } else {
                _charaMatchedIds.value = null
            }
            if (newFilters.speakerText != null) {
                _speakerMatchedIds.value = analysisDao.searchLocalIdsBySpeaker(newFilters.speakerText).toSet()
            } else {
                _speakerMatchedIds.value = null
            }
            if (newFilters.dialogText != null) {
                _textMatchedIds.value = analysisDao.searchLocalIdsByText(newFilters.dialogText).toSet()
            } else {
                _textMatchedIds.value = null
            }
        }
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
