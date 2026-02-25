package moe.ryoha.remoterg.data.repository

import android.content.Context
import android.content.SharedPreferences
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class SettingsRepository @Inject constructor(
    @ApplicationContext private val context: Context
) {
    private val prefs: SharedPreferences = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    private val _useOriginalQualityScreenshot = MutableStateFlow(
        prefs.getBoolean(KEY_USE_ORIGINAL_QUALITY_SCREENSHOT, true)
    )
    val useOriginalQualityScreenshot: StateFlow<Boolean> = _useOriginalQualityScreenshot.asStateFlow()

    fun setUseOriginalQualityScreenshot(useOriginal: Boolean) {
        prefs.edit().putBoolean(KEY_USE_ORIGINAL_QUALITY_SCREENSHOT, useOriginal).apply()
        _useOriginalQualityScreenshot.value = useOriginal
    }

    private val _isShiftButtonEnabled = MutableStateFlow(
        prefs.getBoolean(KEY_IS_SHIFT_BUTTON_ENABLED, false)
    )
    val isShiftButtonEnabled: StateFlow<Boolean> = _isShiftButtonEnabled.asStateFlow()

    fun setShiftButtonEnabled(enabled: Boolean) {
        prefs.edit().putBoolean(KEY_IS_SHIFT_BUTTON_ENABLED, enabled).apply()
        _isShiftButtonEnabled.value = enabled
    }

    private val _isTrackpadModeEnabled = MutableStateFlow(
        prefs.getBoolean(KEY_IS_TRACKPAD_MODE_ENABLED, false)
    )
    val isTrackpadModeEnabled: StateFlow<Boolean> = _isTrackpadModeEnabled.asStateFlow()

    fun setTrackpadModeEnabled(enabled: Boolean) {
        prefs.edit().putBoolean(KEY_IS_TRACKPAD_MODE_ENABLED, enabled).apply()
        _isTrackpadModeEnabled.value = enabled
    }

    companion object {
        private const val PREFS_NAME = "remoterg_settings"
        private const val KEY_USE_ORIGINAL_QUALITY_SCREENSHOT = "use_original_quality_screenshot"
        private const val KEY_IS_SHIFT_BUTTON_ENABLED = "is_shift_button_enabled"
        private const val KEY_IS_TRACKPAD_MODE_ENABLED = "is_trackpad_mode_enabled"
    }
}
