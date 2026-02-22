package expo.modules.pip

import android.app.PictureInPictureParams
import android.os.Build
import android.util.Log
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
        Function("enterPip") { width: Int, height: Int, x: Int, y: Int, w: Int, h: Int ->
            val activity = appContext.currentActivity ?: return@Function null
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val builder = PictureInPictureParams.Builder()
                
                // アスペクト比の設定
                if (width > 0 && height > 0) {
                    builder.setAspectRatio(Rational(width, height))
                }
                
                // ソース矩形の設定（アニメーション用）
                if (w > 0 && h > 0) {
                    val rect = android.graphics.Rect(x, y, x + w, y + h)
                    builder.setSourceRectHint(rect)
                }
                
                activity.enterPictureInPictureMode(builder.build())
            }
            null
        }

        // 自動 PiP の有効/無効を切り替え (Android 12+)
        Function("setAutoEnterEnabled") { enabled: Boolean, width: Int, height: Int, x: Int, y: Int, w: Int, h: Int ->
            val activity = appContext.currentActivity ?: return@Function null
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                Log.d("PipModule", "setAutoEnterEnabled called: enabled=$enabled, aspect=$width/$height, rect_x=$x, rect_y=$y, rect_w=$w, rect_h=$h")
                val builder = PictureInPictureParams.Builder()
                    .setAutoEnterEnabled(enabled)

                if (enabled) {
                    // アスペクト比の設定
                    if (width > 0 && height > 0) {
                        builder.setAspectRatio(Rational(width, height))
                    }
                    
                    // ソース矩形の設定
                    if (w > 0 && h > 0) {
                        val rect = android.graphics.Rect(x, y, x + w, y + h)
                        builder.setSourceRectHint(rect)
                    }
                }
                
                activity.setPictureInPictureParams(builder.build())
            }
            null
        }

        // パラメータのみ更新する関数 (PiPモード中や、自動入室のパラメータ更新用)
        Function("setPipParams") { width: Int, height: Int, x: Int, y: Int, w: Int, h: Int ->
            val activity = appContext.currentActivity ?: return@Function null
             if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Log.d("PipModule", "setPipParams called: aspect=$width/$height, rect_x=$x, rect_y=$y, rect_w=$w, rect_h=$h")
                val builder = PictureInPictureParams.Builder()
                
                if (width > 0 && height > 0) {
                    builder.setAspectRatio(Rational(width, height))
                }
                
                if (w > 0 && h > 0) {
                    val rect = android.graphics.Rect(x, y, x + w, y + h)
                    builder.setSourceRectHint(rect)
                }

                activity.setPictureInPictureParams(builder.build())
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
