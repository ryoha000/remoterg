package moe.ryoha.remoterg.ui.screens

import android.net.Uri
import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import coil.compose.AsyncImagePainter
import coil.request.ImageRequest
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

@Composable
fun ScreenshotFlash(
    viewModel: moe.ryoha.remoterg.ui.viewmodel.ViewerViewModel
) {
    var activeScreenshotUri by remember { mutableStateOf<Uri?>(null) }
    var flashTrigger by remember { androidx.compose.runtime.mutableIntStateOf(0) }
    val context = LocalContext.current

    LaunchedEffect(Unit) {
        launch {
            viewModel.screenshotTriggerFlow.collect {
                flashTrigger++
            }
        }
        launch {
            viewModel.screenshotSavedFlow.collect { uri ->
                activeScreenshotUri = uri
                android.widget.Toast.makeText(context, "Screenshot saved!", android.widget.Toast.LENGTH_SHORT).show()
            }
        }
    }

    val screenshotUri = activeScreenshotUri
    val onAnimationEnd = { activeScreenshotUri = null }

    val configuration = LocalConfiguration.current
    val density = LocalDensity.current
    
    val screenWidthPx = with(density) { configuration.screenWidthDp.dp.toPx() }
    val screenHeightPx = with(density) { configuration.screenHeightDp.dp.toPx() }

    // アニメーション用のダウンサンプリングサイズ（フル画像をデコードしない）
    val thumbnailWidthPx = remember(density) {
        with(density) { (configuration.screenWidthDp.dp / 2).toPx().roundToInt() }
    }
    val thumbnailHeightPx = remember(density) {
        with(density) { (configuration.screenHeightDp.dp / 2).toPx().roundToInt() }
    }

    // Flash アニメーション
    val flashAlpha = remember { Animatable(0f) }
    
    // サムネイル縮小アニメーション
    val imageScale = remember { Animatable(1f) }
    val imageTranslateX = remember { Animatable(0f) }
    val imageTranslateY = remember { Animatable(0f) }
    val imageAlpha = remember { Animatable(1f) }

    // 画像のロード完了フラグ（描画前にアニメーションが始まるのを防ぐ）
    var imageReady by remember { mutableStateOf(false) }

    LaunchedEffect(flashTrigger) {
        if (flashTrigger > 0) {
            launch {
                flashAlpha.snapTo(0.8f)
                val flashDuration = 100
                flashAlpha.animateTo(
                    targetValue = 0f,
                    animationSpec = tween(durationMillis = flashDuration, easing = LinearEasing)
                )
            }
        }
    }

    // screenshotUri が変わったらロード完了フラグをリセット
    LaunchedEffect(screenshotUri) {
        imageReady = false
    }

    LaunchedEffect(screenshotUri, imageReady) {
        if (screenshotUri != null && imageReady) {
            // 画像のロードが完了してからサムネイルアニメーションを開始
            launch {
                imageScale.snapTo(1f)
                imageTranslateX.snapTo(0f)
                imageTranslateY.snapTo(0f)
                imageAlpha.snapTo(1f)

                val targetScale = 0.2f
                val marginPx = with(density) { 20.dp.toPx() }
                
                // 画面サイズを基準に計算（onSizeChanged 不要）
                val currentW = screenWidthPx
                val currentH = screenHeightPx

                // 現在のフルスケール画像の中心
                val currentCenterX = screenWidthPx / 2f
                val currentCenterY = screenHeightPx / 2f

                // ターゲット中心（左下）
                val targetW = currentW * targetScale
                val targetH = currentH * targetScale

                val targetCenterX = marginPx + targetW / 2f
                val targetCenterY = screenHeightPx - marginPx * 2f - targetH / 2f

                val transX = targetCenterX - currentCenterX
                val transY = targetCenterY - currentCenterY

                // アニメーション時間: 1秒
                val shrinkDuration = 1000

                launch {
                    imageScale.animateTo(
                        targetValue = targetScale,
                        animationSpec = tween(durationMillis = shrinkDuration, easing = FastOutSlowInEasing)
                    )
                }
                launch {
                    imageTranslateX.animateTo(
                        targetValue = transX,
                        animationSpec = tween(durationMillis = shrinkDuration, easing = FastOutSlowInEasing)
                    )
                }
                launch {
                    imageTranslateY.animateTo(
                        targetValue = transY,
                        animationSpec = tween(durationMillis = shrinkDuration, easing = FastOutSlowInEasing)
                    )
                }

                // 縮小後の静止状態を2秒間維持
                delay(shrinkDuration.toLong() + 2000)

                imageAlpha.animateTo(
                    targetValue = 0f,
                    animationSpec = tween(durationMillis = 300)
                )
                
                onAnimationEnd()
            }
        }
    }

    // Image Layer — 独立 Composable でリコンポジション範囲を制限
    ScreenshotImageLayer(
        screenshotUri = screenshotUri,
        imageScale = imageScale,
        imageTranslateX = imageTranslateX,
        imageTranslateY = imageTranslateY,
        imageAlpha = imageAlpha,
        thumbnailWidthPx = thumbnailWidthPx,
        thumbnailHeightPx = thumbnailHeightPx,
        onImageReady = { imageReady = true }
    )

    // Flash Layer — 独立 Composable でリコンポジション範囲を制限
    FlashOverlay(flashAlpha = flashAlpha)
}

/**
 * スクリーンショット画像のアニメーション表示レイヤー
 * 独立 Composable にすることで、アニメーション値の読み取りが親に波及しない
 */
@Composable
private fun ScreenshotImageLayer(
    screenshotUri: Uri?,
    imageScale: Animatable<Float, AnimationVector1D>,
    imageTranslateX: Animatable<Float, AnimationVector1D>,
    imageTranslateY: Animatable<Float, AnimationVector1D>,
    imageAlpha: Animatable<Float, AnimationVector1D>,
    thumbnailWidthPx: Int,
    thumbnailHeightPx: Int,
    onImageReady: () -> Unit
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .graphicsLayer {
                scaleX = imageScale.value
                scaleY = imageScale.value
                translationX = imageTranslateX.value
                translationY = imageTranslateY.value
                alpha = imageAlpha.value
            },
        contentAlignment = Alignment.Center
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .clip(RoundedCornerShape(8.dp))
        ) {
            AsyncImage(
                model = ImageRequest.Builder(LocalContext.current)
                    .data(screenshotUri)
                    // アニメーション用にダウンサンプリング: フル画像をデコードするとGPU負荷が高い
                    .size(thumbnailWidthPx, thumbnailHeightPx)
                    .crossfade(false) // アニメーション中のクロスフェードは不要
                    .build(),
                contentDescription = null,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize(),
                onState = { state ->
                    if (state is AsyncImagePainter.State.Success) {
                        onImageReady()
                    }
                }
            )
        }
    }
}

/**
 * フラッシュオーバーレイ
 * 独立 Composable にすることで、flashAlpha の読み取りが親に波及しない
 */
@Composable
private fun FlashOverlay(flashAlpha: Animatable<Float, AnimationVector1D>) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .graphicsLayer { alpha = flashAlpha.value }
            .background(Color.White)
    )
}

