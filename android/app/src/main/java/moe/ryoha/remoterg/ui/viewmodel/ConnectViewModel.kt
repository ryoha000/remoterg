package moe.ryoha.remoterg.ui.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.launch
import moe.ryoha.remoterg.data.repository.ScreenshotRepository
import moe.ryoha.remoterg.data.repository.GoogleDriveRepository
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.MutableSharedFlow
import android.util.Log
import javax.inject.Inject

@HiltViewModel
class ConnectViewModel @Inject constructor(
    private val repository: ScreenshotRepository,
    private val googleDriveRepository: GoogleDriveRepository
) : ViewModel() {

    val isGoogleDriveConnected: StateFlow<Boolean> = googleDriveRepository.isConnected

    private val _googleDriveAuthUrlFlow = MutableSharedFlow<String>(extraBufferCapacity = 1)
    val googleDriveAuthUrlFlow = _googleDriveAuthUrlFlow.asSharedFlow()

    private var currentSignalingUrl: String? = null

    init {
        setupGoogleDriveAuthTracking()
    }

    private fun setupGoogleDriveAuthTracking() {
        viewModelScope.launch {
            googleDriveRepository.authCodeFlow.collect { code ->
                currentSignalingUrl?.let { url ->
                    try {
                        googleDriveRepository.exchangeCodeToTokens(url, code)
                    } catch (e: Exception) {
                        Log.e("ConnectViewModel", "Failed to exchange Google Drive token", e)
                    }
                }
            }
        }
    }

    fun startGoogleDriveAuth(signalingUrl: String) {
        currentSignalingUrl = signalingUrl
        viewModelScope.launch {
            try {
                val url = googleDriveRepository.fetchAuthUrl(signalingUrl)
                _googleDriveAuthUrlFlow.tryEmit(url)
            } catch (e: Exception) {
                Log.e("ConnectViewModel", "Failed to get auth URL", e)
            }
        }
    }

    fun disconnectGoogleDrive() {
        googleDriveRepository.disconnect()
    }

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
