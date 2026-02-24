package moe.ryoha.remoterg.ui.screens

import android.app.PictureInPictureParams
import android.os.Build
import android.util.Rational
import androidx.activity.ComponentActivity
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VectorConverter
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.core.app.PictureInPictureModeChangedInfo
import androidx.core.util.Consumer
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

// ─── OverlayState ───────────────────────────────────────────────

/**
 * オーバーレイの表示/非表示・各パネルの開閉・タイマー制御を集約した State Holder。
 *
 * @Stable を付けることで Compose に「構造的等価性で変化を検出する」ことを伝え、
 * 不要なリコンポジションを抑制する。
 */
@Stable
class OverlayState {
    var showOverlay by mutableStateOf(true)
        private set
    var showDebug by mutableStateOf(false)
    var showSettings by mutableStateOf(false)
    var showDetailedSettings by mutableStateOf(false)
    var showConnectionDetails by mutableStateOf(false)
    var audioVolume by mutableFloatStateOf(1f)

    /** 最終操作時刻（自動非表示タイマーのトリガーに使用） */
    var lastInteraction by mutableLongStateOf(System.currentTimeMillis())
        private set

    /** オーバーレイの表示/非表示を切替え、操作時刻を更新する */
    fun toggleOverlay() {
        showOverlay = !showOverlay
        lastInteraction = System.currentTimeMillis()
    }

    /** ユーザー操作時刻を更新（自動非表示タイマーをリセット） */
    fun onInteraction() {
        lastInteraction = System.currentTimeMillis()
    }

    /** 自動非表示タイマーによりオーバーレイを閉じる */
    fun hideOverlay() {
        showOverlay = false
    }
}

@Composable
fun rememberOverlayState(): OverlayState = remember { OverlayState() }

// ─── ZoomPanState ───────────────────────────────────────────────

/**
 * ピンチズーム・パン・ダブルタップリセットの状態を集約した State Holder。
 *
 * graphicsLayer で読み取る scale / offset と、アニメーション用の Animatable を
 * 同一オブジェクトにまとめることで、状態の同期漏れを防止する。
 */
@Stable
class ZoomPanState {
    var scale by mutableFloatStateOf(1f)
    var offset by mutableStateOf(Offset.Zero)

    val scaleAnimatable = Animatable(1f)
    val offsetAnimatable = Animatable(Offset.Zero, Offset.VectorConverter)

    /** ピンチズーム・パンジェスチャーの処理 */
    fun onTransform(pan: Offset, zoom: Float, coroutineScope: CoroutineScope) {
        val newScale = (scale * zoom).coerceAtLeast(1f)
        val newOffset = if (newScale > 1f) offset + pan else Offset.Zero
        scale = newScale
        offset = newOffset
        // Animatable の内部値も同期（次のアニメーション起点を正しく設定）
        coroutineScope.launch {
            scaleAnimatable.snapTo(newScale)
            offsetAnimatable.snapTo(newOffset)
        }
    }

    /** ダブルタップでズーム・パンをリセット（アニメーション付き） */
    suspend fun resetZoom() {
        kotlinx.coroutines.coroutineScope {
            launch { scaleAnimatable.animateTo(1f) }
            launch { offsetAnimatable.animateTo(Offset.Zero) }
        }
    }
}

@Composable
fun rememberZoomPanState(): ZoomPanState {
    val state = remember { ZoomPanState() }
    // Animatable の値を読み取り可能な state に反映
    LaunchedEffect(state.scaleAnimatable.value) { state.scale = state.scaleAnimatable.value }
    LaunchedEffect(state.offsetAnimatable.value) { state.offset = state.offsetAnimatable.value }
    return state
}

// ─── PiP Controller ─────────────────────────────────────────────

/**
 * PiP (Picture-in-Picture) 関連のサイドエフェクトをカプセル化する Composable。
 *
 * PiP リスナーの登録/解除、アスペクト比の設定、自動 PiP の有効/無効を管理する。
 * 戻り値は現在の PiP モード状態。
 */
@Composable
fun rememberPipState(
    activity: ComponentActivity?,
    frameWidth: Int,
    frameHeight: Int
): Boolean {
    var isInPipMode by remember {
        mutableStateOf(
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N)
                activity?.isInPictureInPictureMode == true
            else false
        )
    }

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        // PiP モード変更リスナーの登録
        DisposableEffect(activity) {
            val listener = Consumer<PictureInPictureModeChangedInfo> { info ->
                isInPipMode = info.isInPictureInPictureMode
            }
            activity?.addOnPictureInPictureModeChangedListener(listener)
            onDispose {
                activity?.removeOnPictureInPictureModeChangedListener(listener)
            }
        }

        // アスペクト比の更新と自動 PiP 有効化
        LaunchedEffect(frameWidth, frameHeight, activity) {
            val width = frameWidth.takeIf { it > 0 } ?: 16
            val height = frameHeight.takeIf { it > 0 } ?: 9

            val builder = PictureInPictureParams.Builder()
            try {
                builder.setAspectRatio(Rational(width, height))
            } catch (e: Exception) {
                builder.setAspectRatio(Rational(16, 9))
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                builder.setAutoEnterEnabled(true)
            }
            try {
                activity?.setPictureInPictureParams(builder.build())
            } catch (_: Exception) {}
        }

        // クリーンアップ時に自動 PiP を無効化
        DisposableEffect(activity) {
            onDispose {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    try {
                        activity?.setPictureInPictureParams(
                            PictureInPictureParams.Builder()
                                .setAutoEnterEnabled(false)
                                .build()
                        )
                    } catch (_: Exception) {}
                }
            }
        }
    }

    return isInPipMode
}
