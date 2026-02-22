package moe.ryoha.remoterg.webrtc

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import org.webrtc.RendererCommon
import org.webrtc.SurfaceViewRenderer
import org.webrtc.VideoTrack

/**
 * WebRTC の映像トラックを Compose 上で表示するコンポーネント
 *
 * object-fit: contain 相当の挙動を実現:
 * - wrapContentSize() を指定し、Compose から SurfaceView に AT_MOST の制約を渡す
 * - EGL レンダリングが映像のアスペクト比に合わせて適切にリサイズされる
 */
@Composable
fun WebRtcVideoRenderer(
    videoTrack: VideoTrack?,
    webRtcManager: WebRtcManager,
    modifier: Modifier = Modifier
) {
    val rendererRef = remember { mutableListOf<SurfaceViewRenderer?>(null) }
    // 前回の VideoTrack を追跡し、addSink の重複呼び出しを防止
    val lastTrackRef = remember { mutableListOf<VideoTrack?>(null) }
    val currentTrack = rememberUpdatedState(videoTrack)

    AndroidView(
        factory = { context ->
            SurfaceViewRenderer(context).apply {
                init(webRtcManager.eglBaseContext, null)
                setScalingType(RendererCommon.ScalingType.SCALE_ASPECT_FIT)
                setEnableHardwareScaler(true)
                rendererRef[0] = this
            }
        },
        update = { view ->
            rendererRef[0] = view
            // トラックが変更された場合のみ sink を更新（リコンポジション毎の重複 addSink を防止）
            val prevTrack = lastTrackRef[0]
            if (prevTrack !== videoTrack) {
                try {
                    prevTrack?.removeSink(view)
                } catch (_: Exception) {
                    // 前のトラックが既に破棄されている場合は無視
                }
                videoTrack?.addSink(view)
                lastTrackRef[0] = videoTrack
            }
        },
        onRelease = { view ->
            try {
                lastTrackRef[0]?.removeSink(view)
            } catch (_: Exception) {}
            view.release()
        },
        modifier = modifier.wrapContentSize(Alignment.Center)
    )

    // コンポーネント破棄時に sink を解除
    DisposableEffect(videoTrack) {
        onDispose {
            rendererRef[0]?.let { renderer ->
                try {
                    currentTrack.value?.removeSink(renderer)
                } catch (e: Exception) {
                    // トラックが既に破棄されている場合は無視
                }
                // release() は onRelease に移動
            }
            lastTrackRef[0] = null
        }
    }
}

