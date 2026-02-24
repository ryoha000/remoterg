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

    companion object {
        private const val PREFS_NAME = "remoterg_settings"
        private const val KEY_USE_ORIGINAL_QUALITY_SCREENSHOT = "use_original_quality_screenshot"
    }
}
