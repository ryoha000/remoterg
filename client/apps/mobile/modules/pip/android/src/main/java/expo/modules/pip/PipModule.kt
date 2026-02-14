package expo.modules.pip

import android.app.PictureInPictureParams
import android.os.Build
import android.util.Rational

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class PipModule : Module() {
    // 現在の PiP 状態を追跡
    private var isCurrentlyInPip = false

    override fun definition() = ModuleDefinition {
        Name("Pip")

        // PiP モード変更イベント
        Events("onPipModeChanged")

        // 手動で PiP モードに入る
        Function("enterPip") {
            val activity = appContext.currentActivity ?: return@Function null
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val params = PictureInPictureParams.Builder()
                    .setAspectRatio(Rational(16, 9))
                    .build()
                activity.enterPictureInPictureMode(params)
            }
            null
        }

        // 自動 PiP の有効/無効を切り替え (Android 12+)
        Function("setAutoEnterEnabled") { enabled: Boolean ->
            val activity = appContext.currentActivity ?: return@Function null
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val params = PictureInPictureParams.Builder()
                    .setAspectRatio(Rational(16, 9))
                    .setAutoEnterEnabled(enabled)
                    .build()
                activity.setPictureInPictureParams(params)
            }
            null
        }

        // 現在 PiP モードかどうかを返す
        Function("isInPipMode") {
            val activity = appContext.currentActivity ?: return@Function false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                return@Function activity.isInPictureInPictureMode
            }
            false
        }

        // Activity がフォアグラウンドに戻った時 (onResume 相当)
        // PiP から復帰した場合はここで検知する
        OnActivityEntersForeground {
            checkPipModeChange()
        }

        // Activity がバックグラウンドに入った時 (onPause 相当)
        // PiP に入った場合はここで検知する
        OnActivityEntersBackground {
            checkPipModeChange()
        }
    }

    /**
     * PiP モードの変更をチェックし、変更があればイベントを発火する
     */
    private fun checkPipModeChange() {
        val activity = appContext.currentActivity ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val inPip = activity.isInPictureInPictureMode
            if (inPip != isCurrentlyInPip) {
                isCurrentlyInPip = inPip
                sendEvent("onPipModeChanged", mapOf("isInPipMode" to inPip))
            }
        }
    }
}
