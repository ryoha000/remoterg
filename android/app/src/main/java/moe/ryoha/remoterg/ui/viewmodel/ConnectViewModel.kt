package moe.ryoha.remoterg.ui.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.launch
import moe.ryoha.remoterg.data.repository.ScreenshotRepository
import javax.inject.Inject

@HiltViewModel
class ConnectViewModel @Inject constructor(
    private val repository: ScreenshotRepository
) : ViewModel() {

    fun clearAllData(onComplete: (Boolean) -> Unit) {
        viewModelScope.launch {
            val success = repository.clearAllScreenshots()
            onComplete(success)
        }
    }

    /** デバッグ用: 全スクリーンショットのサムネイルを一括生成 */
    fun generateAllThumbnails(onComplete: (Int) -> Unit) {
        viewModelScope.launch {
            val count = repository.generateAllThumbnails()
            onComplete(count)
        }
    }
}
