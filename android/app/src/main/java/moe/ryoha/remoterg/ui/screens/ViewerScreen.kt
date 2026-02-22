package moe.ryoha.remoterg.ui.screens

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.CellTower
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import android.widget.Toast
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import android.os.Build
import androidx.activity.ComponentActivity
import androidx.core.app.PictureInPictureModeChangedInfo
import androidx.core.util.Consumer
import moe.ryoha.remoterg.webrtc.WebRtcStats
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import moe.ryoha.remoterg.ui.viewmodel.ViewerViewModel
import moe.ryoha.remoterg.webrtc.WebRtcVideoRenderer

/**
 * 映像表示画面
 *
 * RN の ViewerScreen + ViewerOverlay のデザインを再現。
 * - タップでオーバーレイの表示/非表示を切替
 * - 4秒後に自動的に非表示
 * - トップバー: 戻るボタン、ステータスバッジ、loss バッジ、右側にデバッグ/ギャラリー/カメラ/設定ボタン
 * - デバッグパネル: FPS/Bitrate/Loss/Session 表示
 * - 設定パネル: Audio ボリューム、Disconnect ボタン
 */
@Composable
fun ViewerScreen(
    signalingUrl: String,
    codec: String,
    viewModel: ViewerViewModel,
    onNavigateBack: () -> Unit,
    onNavigateToGallery: () -> Unit,
) {
    val context = LocalContext.current
    val videoTrack by viewModel.webRtcManager.remoteVideoTrack.collectAsState()
    val isConnected by viewModel.webRtcManager.isConnected.collectAsState()
    val rtcStats by viewModel.rtcStats.collectAsState()
    
    val activity = context as? ComponentActivity
    var isInPipMode by remember { mutableStateOf(if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) activity?.isInPictureInPictureMode == true else false) }

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        DisposableEffect(activity) {
            val listener = Consumer<PictureInPictureModeChangedInfo> { info ->
                isInPipMode = info.isInPictureInPictureMode
            }
            activity?.addOnPictureInPictureModeChangedListener(listener)
            onDispose {
                activity?.removeOnPictureInPictureModeChangedListener(listener)
            }
        }

        LaunchedEffect(rtcStats.frameWidth, rtcStats.frameHeight, activity) {
            val width = rtcStats.frameWidth.takeIf { it > 0 } ?: 16
            val height = rtcStats.frameHeight.takeIf { it > 0 } ?: 9

            val builder = android.app.PictureInPictureParams.Builder()
            try {
                // アスペクト比が極端な場合はクラッシュ防止のためデフォルト値を設定
                builder.setAspectRatio(android.util.Rational(width, height))
            } catch (e: Exception) {
                builder.setAspectRatio(android.util.Rational(16, 9))
            }
            
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                builder.setAutoEnterEnabled(true)
            }
            try {
                activity?.setPictureInPictureParams(builder.build())
            } catch (e: Exception) {
            }
        }

        DisposableEffect(activity) {
            onDispose {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    try {
                        activity?.setPictureInPictureParams(
                            android.app.PictureInPictureParams.Builder()
                                .setAutoEnterEnabled(false)
                                .build()
                        )
                    } catch (e: Exception) {}
                }
            }
        }
    }
    
    val displayMetrics = context.resources.displayMetrics
    val deviceScreenSize = "${displayMetrics.widthPixels}x${displayMetrics.heightPixels}"

    // オーバーレイ状態
    var showOverlay by remember { mutableStateOf(true) }
    var lastInteraction by remember { mutableLongStateOf(System.currentTimeMillis()) }
    var showDebug by remember { mutableStateOf(false) }
    var showConnectionDetails by remember { mutableStateOf(false) }
    var showSettings by remember { mutableStateOf(false) }
    var audioVolume by remember { mutableFloatStateOf(1f) }

    // ピンチズーム / パン 状態
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    val scaleAnimatable = remember { Animatable(1f) }
    val offsetAnimatable = remember { Animatable(Offset.Zero, Offset.VectorConverter) }
    val coroutineScope = rememberCoroutineScope()

    // Animatable の値を scale / offset に反映
    LaunchedEffect(scaleAnimatable.value) { scale = scaleAnimatable.value }
    LaunchedEffect(offsetAnimatable.value) { offset = offsetAnimatable.value }

    LaunchedEffect(signalingUrl) {
        viewModel.connectToHost(signalingUrl, codec)
    }

    // Removed DisposableEffect that calls disconnect() so navigation to Gallery doesn't disconnect WebRTC

    // オーバーレイの自動非表示（4秒）
    LaunchedEffect(showOverlay, lastInteraction) {
        if (showOverlay && isConnected) {
            delay(4000)
            showOverlay = false
        }
    }

    val toggleOverlay = {
        showOverlay = !showOverlay
        lastInteraction = System.currentTimeMillis()
    }

    val onInteraction = {
        lastInteraction = System.currentTimeMillis()
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
            // ピンチズーム & パン ジェスチャー
            .pointerInput(Unit) {
                detectTransformGestures { _, pan, zoom, _ ->
                    val newScale = (scale * zoom).coerceAtLeast(1f)
                    // ズーム中のみパンを許可（等倍時はパン不可）
                    val newOffset = if (newScale > 1f) {
                        offset + pan
                    } else {
                        Offset.Zero
                    }
                    scale = newScale
                    offset = newOffset
                    // Animatable の内部値も同期（次のアニメーション起点を正しく設定）
                    coroutineScope.launch {
                        scaleAnimatable.snapTo(newScale)
                        offsetAnimatable.snapTo(newOffset)
                    }
                }
            }
            // シングルタップ & ダブルタップ ジェスチャー
            .pointerInput(Unit) {
                detectTapGestures(
                    onDoubleTap = {
                        // ダブルタップ: ズームとパンをリセット（アニメーション付き）
                        coroutineScope.launch {
                            launch { scaleAnimatable.animateTo(1f) }
                            launch { offsetAnimatable.animateTo(Offset.Zero) }
                        }
                    },
                    onTap = {
                        // シングルタップ: 設定パネルが開いている場合は閉じる、そうでなければオーバーレイ切替
                        if (showSettings) {
                            showSettings = false
                        } else {
                            toggleOverlay()
                        }
                    }
                )
            }
    ) {
        // 映像表示エリア（ズーム & パン適用）
        Box(
            modifier = Modifier
                .fillMaxSize()
                .graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    translationX = offset.x
                    translationY = offset.y
                }
        ) {
            if (videoTrack != null) {
                WebRtcVideoRenderer(
                    videoTrack = videoTrack,
                    webRtcManager = viewModel.webRtcManager,
                    modifier = Modifier.fillMaxSize()
                )
            } else {
                // 接続中のプレースホルダ
                Text(
                    text = if (isConnected) "映像トラック待機中..." else "接続中...",
                    color = Color.White,
                    modifier = Modifier.align(Alignment.Center)
                )
            }
        }

        // === オーバーレイ ===
        if (!isInPipMode) {
            val overlayTopPadding by animateDpAsState(
                targetValue = if (showOverlay) 72.dp else 16.dp,
                label = "overlayTopPadding"
            )

            // トップバー
            AnimatedVisibility(
                visible = showOverlay,
                enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                modifier = Modifier.align(Alignment.TopCenter)
            ) {
                TopBar(
                    isConnected = isConnected,
                    rtcStats = rtcStats,
                    onBack = {
                        viewModel.disconnect()
                        onNavigateBack()
                    },
                    showSettings = showSettings,
                    onToggleSettings = {
                        showSettings = !showSettings
                        onInteraction()
                    },
                    onScreenshot = {
                        viewModel.takeScreenshot()
                        onInteraction()
                    },
                    onNavigateToGallery = {
                        onNavigateToGallery() // External navigation
                    },
                    onInteraction = onInteraction
                )
            }

            // デバッグパネル（左側）
            AnimatedVisibility(
                visible = showDebug,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(start = 16.dp, top = overlayTopPadding)
            ) {
                DebugPanel(
                    rtcStats = rtcStats,
                    deviceScreenSize = deviceScreenSize
                )
            }

            // 設定パネル（右側）
            AnimatedVisibility(
                visible = showSettings,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(end = 16.dp, top = overlayTopPadding)
            ) {
                SettingsPanel(
                    volume = audioVolume,
                    onVolumeChange = { vol ->
                        audioVolume = vol
                        viewModel.webRtcManager.setAudioVolume(vol.toDouble())
                    },
                    showDebug = showDebug,
                    onToggleDebug = {
                        showDebug = it
                        onInteraction()
                    },
                    onShowConnectionDetails = {
                        showConnectionDetails = true
                        onInteraction()
                        showSettings = false
                    },
                    onDisconnect = {
                        viewModel.disconnect()
                        onNavigateBack()
                    }
                )
            }
        }

        // Screen Flash and Thumbnail animation
        ScreenshotFlash(
            viewModel = viewModel
        )
    }

    // 接続詳細情報ダイアログ
    if (showConnectionDetails) {
        val connectionState by viewModel.connectionState.collectAsState()
        val selectedCodec by viewModel.selectedCodec.collectAsState()
        
        // Extract session ID from signalingUrl (assuming format ...?session_id=UUID&...)
        val sessionId = remember(signalingUrl) {
            try {
                val uri = android.net.Uri.parse(signalingUrl)
                uri.getQueryParameter("session_id") ?: "Unknown"
            } catch (e: Exception) {
                "Unknown"
            }
        }
        
        val iceConnectionState by viewModel.webRtcManager.iceConnectionState.collectAsState(initial = "NEW")
        val signalingState by viewModel.webRtcManager.signalingState.collectAsState(initial = "NEW")
        
        ConnectionDetailsDialog(
            rtcStats = rtcStats,
            deviceScreenSize = deviceScreenSize,
            connectionState = connectionState,
            iceConnectionState = iceConnectionState,
            signalingState = signalingState,
            selectedCodec = selectedCodec,
            sessionId = sessionId,
            onDismiss = { showConnectionDetails = false }
        )
    }
}

/**
 * トップバー: 戻るボタン、ステータスバッジ、アクションボタン群
 */
@Composable
private fun TopBar(
    isConnected: Boolean,
    rtcStats: WebRtcStats,
    onBack: () -> Unit,
    showSettings: Boolean,
    onToggleSettings: () -> Unit,
    onScreenshot: () -> Unit,
    onNavigateToGallery: () -> Unit,
    onInteraction: () -> Unit
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color.Black.copy(alpha = 0.4f))
            .padding(horizontal = 12.dp, vertical = 8.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween
        ) {
            // 左側: 戻るボタン + ステータスバッジ
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                // 戻るボタン
                IconButton(
                    onClick = onBack,
                    modifier = Modifier.size(40.dp)
                ) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                        contentDescription = "戻る",
                        tint = Color.White,
                        modifier = Modifier.size(24.dp)
                    )
                }

                // ステータスバッジ
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier
                        .background(
                            Color.Black.copy(alpha = 0.4f),
                            RoundedCornerShape(50)
                        )
                        .border(
                            1.dp,
                            Color.White.copy(alpha = 0.1f),
                            RoundedCornerShape(50)
                        )
                        .padding(horizontal = 12.dp, vertical = 6.dp)
                ) {
                    // インジケーター丸
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .clip(CircleShape)
                            .background(
                                if (isConnected) Color(0xFF22C55E) else Color(0xFFEAB308)
                            )
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        text = if (isConnected) "connected" else "connecting",
                        color = Color.White.copy(alpha = 0.9f),
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium
                    )
                }
            }

            // 右側: アクションボタン群
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                // ギャラリーボタン
                OverlayIconButton(
                    icon = Icons.Default.Image,
                    contentDescription = "ギャラリー",
                    onClick = onNavigateToGallery
                )
                // スクリーンショットボタン
                OverlayIconButton(
                    icon = Icons.Default.CameraAlt,
                    contentDescription = "スクリーンショット",
                    onClick = onScreenshot
                )
                // 設定ボタン
                OverlayIconButton(
                    icon = Icons.Default.Settings,
                    contentDescription = "設定",
                    isActive = showSettings,
                    onClick = onToggleSettings
                )
            }
        }
    }
}

/**
 * オーバーレイ用アイコンボタン
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun OverlayIconButton(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    contentDescription: String,
    isActive: Boolean = false,
    onClick: () -> Unit,
    onLongClick: (() -> Unit)? = null
) {
    Box(
        modifier = Modifier
            .size(40.dp)
            .clip(CircleShape)
            .then(
                if (isActive) Modifier.background(Color.White.copy(alpha = 0.2f))
                else Modifier
            )
            .combinedClickable(
                onClick = onClick,
                onLongClick = onLongClick
            ),
        contentAlignment = Alignment.Center
    ) {
        Icon(
            imageVector = icon,
            contentDescription = contentDescription,
            tint = Color.White,
            modifier = Modifier.size(20.dp)
        )
    }
}

/**
 * デバッグパネル: 頻繁に変わる統計情報を表示 (FPS/Bitrate/Loss)
 */
@Composable
private fun DebugPanel(
    rtcStats: WebRtcStats,
    deviceScreenSize: String
) {
    Column(
        modifier = Modifier
            .width(200.dp) // 幅を制限して拡がりすぎないようにする
            .background(
                Color.Black.copy(alpha = 0.6f),
                RoundedCornerShape(8.dp)
            )
            .border(
                1.dp,
                Color.White.copy(alpha = 0.1f),
                RoundedCornerShape(8.dp)
            )
            .padding(12.dp)
    ) {
        val monoStyle = androidx.compose.ui.text.TextStyle(
            color = Color(0xFF4ADE80), // green-400
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace
        )
        Row(modifier = Modifier.fillMaxWidth()) {
            Text(text = "FPS", style = monoStyle, modifier = Modifier.width(72.dp))
            Text(text = "${rtcStats.fps}", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
        }
        Row(modifier = Modifier.fillMaxWidth()) {
            Text(text = "Bitrate", style = monoStyle, modifier = Modifier.width(72.dp))
            Text(text = "${rtcStats.bitrate} kbps", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
        }
        Row(modifier = Modifier.fillMaxWidth()) {
            Text(text = "Loss", style = monoStyle, modifier = Modifier.width(72.dp))
            Text(text = "${rtcStats.loss}%", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
        }
    }
}

/**
 * 通信詳細ダイアログ: 静的な接続情報等の詳細を表示
 */
@Composable
private fun ConnectionDetailsDialog(
    rtcStats: WebRtcStats,
    deviceScreenSize: String,
    connectionState: String,
    iceConnectionState: String,
    signalingState: String,
    selectedCodec: String,
    sessionId: String,
    onDismiss: () -> Unit
) {
    androidx.compose.ui.window.Dialog(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .width(320.dp) // 情報が増えるため少し幅を広げる
                .background(Color(0xFF18181B), RoundedCornerShape(12.dp))
                .border(1.dp, Color(0xFF27272A), RoundedCornerShape(12.dp))
                .padding(16.dp)
        ) {
            Text(
                text = "Connection Details",
                color = Color.White,
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp,
                modifier = Modifier.padding(bottom = 16.dp)
            )

            val labelStyle = androidx.compose.ui.text.TextStyle(
                color = Color(0xFFA1A1AA), // zinc-400
                fontSize = 12.sp
            )
            val valueStyle = androidx.compose.ui.text.TextStyle(
                color = Color.White,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace
            )

            val items = listOf(
                "App State" to connectionState,
                "WebRTC" to signalingState,
                "ICE" to iceConnectionState,
                "Codec" to selectedCodec,
                "Device" to deviceScreenSize,
                "Stream" to "${rtcStats.frameWidth}x${rtcStats.frameHeight}",
                "Session" to sessionId
            )

            items.forEach { (label, value) ->
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text(text = label, style = labelStyle)
                    Text(
                        text = value,
                        style = valueStyle,
                        // Session ID などが長すぎる場合は省略
                        maxLines = 1,
                        modifier = Modifier.padding(start = 16.dp),
                        textAlign = androidx.compose.ui.text.style.TextAlign.End
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Button(
                onClick = onDismiss,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF3F3F46))
            ) {
                Text("Close", color = Color.White)
            }
        }
    }
}

/**
 * 設定パネル: Audio ボリュームスライダー、Disconnect ボタン
 */
@Composable
private fun SettingsPanel(
    volume: Float,
    onVolumeChange: (Float) -> Unit,
    showDebug: Boolean,
    onToggleDebug: (Boolean) -> Unit,
    onShowConnectionDetails: () -> Unit,
    onDisconnect: () -> Unit
) {
    Column(
        modifier = Modifier
            .width(256.dp)
            .background(
                Color(0xFF18181B).copy(alpha = 0.95f), // zinc-900
                RoundedCornerShape(12.dp)
            )
            .border(
                1.dp,
                Color(0xFF27272A), // zinc-800
                RoundedCornerShape(12.dp)
            )
            .padding(16.dp)
    ) {
        // ヘッダー
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "Settings",
                color = Color.White,
                fontWeight = FontWeight.Medium,
                fontSize = 14.sp
            )
            Text(
                text = "v0.1.0",
                color = Color(0xFF71717A), // zinc-500
                fontSize = 10.sp
            )
        }

        Spacer(modifier = Modifier.height(16.dp))

        // Audio セクション（モック）
        Text(
            text = "Audio",
            color = Color(0xFFA1A1AA), // zinc-400
            fontSize = 12.sp,
            modifier = Modifier.padding(bottom = 8.dp)
        )
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.fillMaxWidth()
        ) {
            Icon(
                imageVector = Icons.AutoMirrored.Filled.VolumeUp,
                contentDescription = null,
                tint = Color(0xFFA1A1AA),
                modifier = Modifier.size(16.dp)
            )
            Spacer(modifier = Modifier.width(8.dp))
            // 音量スライダー
            Slider(
                value = volume,
                onValueChange = onVolumeChange,
                valueRange = 0f..1f,
                modifier = Modifier.weight(1f),
                colors = SliderDefaults.colors(
                    thumbColor = Color.White,
                    activeTrackColor = Color(0xFF3B82F6), // blue-500
                    inactiveTrackColor = Color(0xFF3F3F46) // zinc-700
                )
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = "${(volume * 100).toInt()}%",
                color = Color(0xFF71717A),
                fontSize = 12.sp
            )
        }

        Spacer(modifier = Modifier.height(16.dp))

        // 詳細情報・デバッグ設定
        Row(
            modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "Stats Overlay",
                color = Color(0xFFA1A1AA), // zinc-400
                fontSize = 12.sp
            )
            Switch(
                checked = showDebug,
                onCheckedChange = onToggleDebug,
                colors = SwitchDefaults.colors(
                    checkedThumbColor = Color.White,
                    checkedTrackColor = Color(0xFF3B82F6), // blue-500
                    uncheckedThumbColor = Color(0xFFA1A1AA), // zinc-400
                    uncheckedTrackColor = Color(0xFF3F3F46), // zinc-700
                    uncheckedBorderColor = Color.Transparent
                )
            )
        }

        Button(
            onClick = onShowConnectionDetails,
            modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp),
            shape = RoundedCornerShape(8.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = Color(0xFF3F3F46) // zinc-700
            )
        ) {
            Text(
                text = "詳細情報を確認する",
                color = Color.White,
                fontSize = 12.sp
            )
        }

        // Disconnect ボタン
        Button(
            onClick = onDisconnect,
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(8.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = Color(0xFFDC2626) // red-600
            )
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center
            ) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.Logout,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(16.dp)
                )
                Spacer(modifier = Modifier.width(8.dp))
                Text(
                    text = "Disconnect",
                    color = Color.White,
                    fontSize = 12.sp
                )
            }
        }
    }
}
