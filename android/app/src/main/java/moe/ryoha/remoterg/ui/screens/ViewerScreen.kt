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
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.ui.draw.rotate
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.gestures.detectDragGestures
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
import androidx.compose.material.icons.filled.ErrorOutline
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.material.icons.filled.Mouse
import androidx.compose.material.icons.filled.TouchApp
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.animation.animateColorAsState
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import android.graphics.Bitmap
import android.widget.Toast
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import android.os.Build
import android.os.SystemClock
import androidx.activity.ComponentActivity
import androidx.core.app.PictureInPictureModeChangedInfo
import androidx.core.util.Consumer
import moe.ryoha.remoterg.webrtc.WebRtcStats
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
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
    val density = LocalDensity.current.density
    val videoTrack by viewModel.remoteVideoTrack.collectAsState()
    val isConnected by viewModel.isConnected.collectAsState()
    val rtcStats by viewModel.rtcStats.collectAsState()
    val connectionState by viewModel.connectionState.collectAsState()
    val webSocketState by viewModel.webSocketState.collectAsState()
    val connectionError by viewModel.connectionError.collectAsState()
    
    val activity = context as? ComponentActivity

    // PiP 状態管理（リスナー登録・アスペクト比設定・自動PiP を State Holder にカプセル化）
    val isInPipMode = rememberPipState(
        activity = activity,
        frameWidth = rtcStats.frameWidth,
        frameHeight = rtcStats.frameHeight
    )

    val displayMetrics = context.resources.displayMetrics
    val deviceScreenSize = "${displayMetrics.widthPixels}x${displayMetrics.heightPixels}"

    // オーバーレイ状態（State Holder に集約）
    val overlayState = rememberOverlayState()

    // ピンチズーム / パン 状態（State Holder に集約）
    val zoomPanState = rememberZoomPanState()
    val coroutineScope = rememberCoroutineScope()
    
    // ジェスチャー中の最大ポインター数をトラッキングして右クリック判定に使用
    val maxPointers = remember { intArrayOf(1) }

    val isTrackpadModeEnabled by viewModel.isTrackpadModeEnabled.collectAsState()

    // SurfaceViewRendererの参照を保持（ローカルスクリーンショット用）
    var surfaceViewRenderer by remember { mutableStateOf<org.webrtc.SurfaceViewRenderer?>(null) }
    val statsGraphHistory = remember { mutableStateListOf<StatsGraphPoint>() }
    val latestRtcStats by rememberUpdatedState(rtcStats)
    var latestLatencySample by remember { mutableStateOf<moe.ryoha.remoterg.webrtc.LatencySample?>(null) }

    // CSV エクスポート用の状態
    var showCsvExportDialog by remember { mutableStateOf(false) }

    val csvExportLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.CreateDocument("text/csv")
    ) { uri ->
        if (uri != null) {
            viewModel.exportLatencyCsv(uri)
            Toast.makeText(context, "Latency data exported successfully", Toast.LENGTH_SHORT).show()
        }
    }

    LaunchedEffect(signalingUrl) {
        viewModel.connectToHost(signalingUrl, codec)
    }

    // オーバーレイの自動非表示（4秒）
    LaunchedEffect(overlayState.showOverlay, overlayState.lastInteraction, isConnected) {
        if (overlayState.showOverlay && isConnected) {
            delay(4000)
            overlayState.hideOverlay()
        }
    }

    // ローカルキャプチャのトリガ処理
    LaunchedEffect(Unit) {
        viewModel.localScreenshotTriggerFlow.collect {
            val svr = surfaceViewRenderer
            if (svr != null) {
                val listener = object : org.webrtc.EglRenderer.FrameListener {
                    override fun onFrame(frameBitmap: Bitmap) {
                        // コールバックは一度だけ受け取る。EglRenderer スレッド以外で removeFrameListener を呼ぶ必要がある
                        android.os.Handler(android.os.Looper.getMainLooper()).post {
                            try {
                                svr.removeFrameListener(this)
                            } catch (e: Exception) {
                                // Ignore
                            }
                        }
                        // ViewModel 経由で保存 (ViewModel 側で IO スレッド等に切り替えて処理)
                        viewModel.saveLocalScreenshot(frameBitmap)
                    }
                }
                // scale = 1.0f を指定することで、ビデオストリーム本来の解像度で Bitmap を取得できる
                svr.addFrameListener(listener, 1.0f)
            }
        }
    }

    // 直近30秒の統計履歴を500msごとにサンプリングし、その間の平均値をプロットする
    LaunchedEffect(isConnected) {
        if (!isConnected) {
            statsGraphHistory.clear()
            return@LaunchedEffect
        }

        var sumLatency = 0L
        var countLatency = 0
        var sumRtt = 0L
        var countRtt = 0
        var sumClockOffset = 0L
        var countClockOffset = 0
        val mutex = Mutex()

        launch {
            viewModel.latencySamples.collect { sample ->
                latestLatencySample = sample
                mutex.withLock {
                    sumLatency += sample.latencyMs
                    countLatency++
                    sumRtt += sample.rttMs
                    countRtt++
                    sumClockOffset += sample.clockOffsetMs
                    countClockOffset++
                }
            }
        }

        while (isConnected) {
            delay(500)
            val now = SystemClock.elapsedRealtime()
            var avgLatency = latestRtcStats.latencyMs
            var avgRtt = latestRtcStats.rttMs
            var avgClockOffset = latestRtcStats.clockOffsetMs

            mutex.withLock {
                if (countLatency > 0) {
                    avgLatency = (sumLatency / countLatency).toInt()
                    avgRtt = (sumRtt / countRtt).toInt()
                    avgClockOffset = (sumClockOffset / countClockOffset).toInt()
                }
                sumLatency = 0L
                countLatency = 0
                sumRtt = 0L
                countRtt = 0
                sumClockOffset = 0L
                countClockOffset = 0
            }

            statsGraphHistory.add(
                StatsGraphPoint(
                    timestampMs = now,
                    rttMs = avgRtt,
                    clockOffsetMs = avgClockOffset,
                    latencyMs = avgLatency
                )
            )
            val cutoff = now - STATS_GRAPH_WINDOW_MS
            while (statsGraphHistory.isNotEmpty() && statsGraphHistory.first().timestampMs < cutoff) {
                statsGraphHistory.removeAt(0)
            }
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
            // ポインター数のトラッキング（二本指タップ判定用）
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial)
                        val pressedCount = event.changes.count { it.pressed }
                        val anyNewDown = event.changes.any { it.pressed && !it.previousPressed }
                        
                        if (anyNewDown && pressedCount == 1) {
                            maxPointers[0] = 1
                        } else if (pressedCount > maxPointers[0]) {
                            maxPointers[0] = pressedCount
                        }
                    }
                }
            }
            // ジェスチャー処理 (Touch / Trackpad 共通)
            .pointerInput(isTrackpadModeEnabled) {
                var accumulatedPanX = 0f
                var accumulatedPanY = 0f
                
                detectTransformGestures { _, pan, zoom, _ ->
                    if (isTrackpadModeEnabled) {
                        // トラックパッドモード: ズームはそのまま、パンはカーソル移動として処理
                        zoomPanState.onTransform(androidx.compose.ui.geometry.Offset.Zero, zoom, coroutineScope)
                        if (!overlayState.showSettings && pan != androidx.compose.ui.geometry.Offset.Zero) {
                            // カーソル感度調整 (タッチスクリーンの移動量を1.5倍に拡張)
                            val sensitivity = 1.5f
                            accumulatedPanX += pan.x * sensitivity
                            accumulatedPanY += pan.y * sensitivity
                            
                            val dxInt = accumulatedPanX.toInt()
                            val dyInt = accumulatedPanY.toInt()
                            
                            if (dxInt != 0 || dyInt != 0) {
                                viewModel.sendCursorMove(dxInt, dyInt)
                                accumulatedPanX -= dxInt
                                accumulatedPanY -= dyInt
                            }
                        }
                    } else {
                        // タッチモード: 通常のパン＆ズーム
                        zoomPanState.onTransform(pan, zoom, coroutineScope)
                    }
                }
            }
            // シングルタップ & ダブルタップ ジェスチャー
            .pointerInput(Unit) {
                detectTapGestures(
                    onDoubleTap = {
                        coroutineScope.launch {
                            zoomPanState.resetZoom()
                        }
                    },
                    onTap = { offset ->
                        // 1. 設定パネルが開いている場合は閉じる
                        if (overlayState.showSettings) {
                            overlayState.showSettings = false
                            return@detectTapGestures
                        }

                        val containerWidth = this.size.width.toFloat()
                        val containerHeight = this.size.height.toFloat()
                        val videoWidth = rtcStats.frameWidth.toFloat()
                        val videoHeight = rtcStats.frameHeight.toFloat()
                        
                        // ヘッダー領域の判定（上部 80dp分）
                        val headerHeightPx = 80f * density
                        val isInHeaderArea = offset.y < headerHeightPx
                        
                        var isInsideVideoBounds = false

                        if (videoWidth > 0f && videoHeight > 0f && containerWidth > 0f && containerHeight > 0f) {
                            val videoRatio = videoWidth / videoHeight
                            val containerRatio = containerWidth / containerHeight
                            
                            var drawWidth = containerWidth
                            var drawHeight = containerHeight
                            var startX = 0f
                            var startY = 0f
                            
                            if (containerRatio > videoRatio) {
                                drawWidth = containerHeight * videoRatio
                                startX = (containerWidth - drawWidth) / 2f
                            } else {
                                drawHeight = containerWidth / videoRatio
                                startY = (containerHeight - drawHeight) / 2f
                            }
                            
                            val scale = zoomPanState.scale
                            val panX = zoomPanState.offset.x
                            val panY = zoomPanState.offset.y
                            val centerX = containerWidth / 2f
                            val centerY = containerHeight / 2f
                            
                            val originalX = (offset.x - centerX - panX) / scale + centerX
                            val originalY = (offset.y - centerY - panY) / scale + centerY
                            
                            val relativeX = originalX - startX
                            val relativeY = originalY - startY
                            
                            if (relativeX in 0f..drawWidth && relativeY in 0f..drawHeight) {
                                isInsideVideoBounds = true
                                // ヘッダー領域でなければクリックイベントを送信
                                if (!isInHeaderArea) {
                                    val button = if (maxPointers[0] >= 2) "right" else "left"
                                    if (isTrackpadModeEnabled) {
                                        viewModel.sendCursorClick(button)
                                    } else {
                                        val normalizedX = relativeX / drawWidth
                                        val normalizedY = relativeY / drawHeight
                                        viewModel.sendMouseClick(normalizedX, normalizedY, button)
                                    }
                                }
                            }
                        }

                        // 2. ヘッダー領域、またはビデオ領域外（黒帯部分）をタップした場合のみ、オーバーレイを表示/非表示切り替え
                        if (isInHeaderArea || !isInsideVideoBounds) {
                            overlayState.toggleOverlay()
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
                    scaleX = zoomPanState.scale
                    scaleY = zoomPanState.scale
                    translationX = zoomPanState.offset.x
                    translationY = zoomPanState.offset.y
                }
        ) {
            if (connectionState == "Failed") {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                    modifier = Modifier.align(Alignment.Center)
                ) {
                    Icon(
                        imageVector = Icons.Default.ErrorOutline,
                        contentDescription = "Error",
                        tint = Color(0xFFEF4444), // red-500
                        modifier = Modifier.size(48.dp)
                    )
                    Spacer(modifier = Modifier.height(16.dp))
                    Text(
                        text = "Connection Failed",
                        color = Color.White,
                        fontWeight = FontWeight.Bold,
                        fontSize = 18.sp
                    )
                    if (connectionError != null) {
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = connectionError ?: "",
                            color = Color(0xFFA1A1AA), // zinc-400
                            fontSize = 14.sp,
                            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                            modifier = Modifier.padding(horizontal = 32.dp)
                        )
                    }
                    Spacer(modifier = Modifier.height(24.dp))
                    Button(
                        onClick = { viewModel.connectToHost(signalingUrl, codec) },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary)
                    ) {
                        Text("Retry")
                    }
                }
            } else if (videoTrack != null) {
                WebRtcVideoRenderer(
                    videoTrack = videoTrack,
                    eglBaseContext = viewModel.eglBaseContext,
                    modifier = Modifier.fillMaxSize(),
                    onSurfaceViewCreated = { surfaceViewRenderer = it },
                    onSurfaceViewDestroyed = { if (surfaceViewRenderer == it) surfaceViewRenderer = null }
                )
            } else if (connectionState == "Disconnected") {
                // 何も表示しない（画面遷移中のため）
                Box(modifier = Modifier.fillMaxSize().background(Color.Black))
            } else {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                    modifier = Modifier.align(Alignment.Center)
                ) {
                    CircularProgressIndicator(
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(32.dp),
                        strokeWidth = 3.dp
                    )
                    Spacer(modifier = Modifier.height(24.dp))
                    
                    // Connection State List
                    Column(
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                        horizontalAlignment = Alignment.Start
                    ) {
                        // 1. Signaling WebScoket State
                        ConnectionPhaseRow(
                            label = "Signaling Server",
                            state = webSocketState
                        )
                        
                        // 2. WebRTC Peer Connection State
                        ConnectionPhaseRow(
                            label = "Peer Connection",
                            state = connectionState
                        )
                        
                        // 3. ICE Connection State
                        ConnectionPhaseRow(
                            label = "ICE Transport",
                            state = viewModel.iceConnectionState.collectAsState().value
                        )
                    }
                }
            }
        }

        // === オーバーレイ ===
        if (!isInPipMode) {
            val overlayTopPadding by animateDpAsState(
                targetValue = if (overlayState.showOverlay) 72.dp else 16.dp,
                label = "overlayTopPadding"
            )

            // トップバー
            AnimatedVisibility(
                visible = overlayState.showOverlay,
                enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                modifier = Modifier.align(Alignment.TopCenter)
            ) {
                TopBar(
                    isConnected = isConnected,
                    connectionState = connectionState,
                    rtcStats = rtcStats,
                    isTrackpadModeEnabled = isTrackpadModeEnabled,
                    onBack = {
                        viewModel.disconnect()
                        onNavigateBack()
                    },
                    showSettings = overlayState.showSettings,
                    onToggleSettings = {
                        overlayState.showSettings = !overlayState.showSettings
                        overlayState.onInteraction()
                    },
                    onNavigateToGallery = {
                        onNavigateToGallery()
                    },
                    onToggleTrackpadMode = {
                        viewModel.setTrackpadModeEnabled(!isTrackpadModeEnabled)
                        overlayState.onInteraction()
                    },
                    onInteraction = { overlayState.onInteraction() }
                )
            }

            // デバッグパネル（左側）
            AnimatedVisibility(
                visible = overlayState.showDebug,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(start = 16.dp, top = overlayTopPadding)
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    DebugPanel(
                        rtcStats = rtcStats,
                        latestLatencySample = latestLatencySample,
                        deviceScreenSize = deviceScreenSize
                    )
                    StatsGraphPanel(
                        rtcStats = rtcStats,
                        history = statsGraphHistory,
                        onClick = { showCsvExportDialog = true }
                    )
                }
            }

            // 設定パネル（右側）
            AnimatedVisibility(
                visible = overlayState.showSettings,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(end = 16.dp, top = overlayTopPadding)
            ) {
                SettingsPanel(
                    volume = overlayState.audioVolume,
                    onVolumeChange = { vol ->
                        overlayState.audioVolume = vol
                        viewModel.setAudioVolume(vol.toDouble())
                    },
                    showDebug = overlayState.showDebug,
                    onToggleDebug = {
                        overlayState.showDebug = it
                        overlayState.onInteraction()
                    },
                    onShowConnectionDetails = {
                        overlayState.showConnectionDetails = true
                        overlayState.onInteraction()
                        overlayState.showSettings = false
                    },
                    onShowDetailedSettings = {
                        overlayState.showDetailedSettings = true
                        overlayState.onInteraction()
                        overlayState.showSettings = false
                    },
                    onDisconnect = {
                        viewModel.disconnect()
                        onNavigateBack()
                    }
                )
            }

            // スクリーンショット / Ctrl(Shift) ボタン (右下)
            AnimatedVisibility(
                visible = overlayState.showOverlay,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(end = 16.dp, bottom = 16.dp)
            ) {
                Column(
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    ScreenshotButton(
                        onInteraction = { overlayState.onInteraction() },
                        onClick = { viewModel.takeScreenshot() }
                    )

                    val isShift by viewModel.isShiftButtonEnabled.collectAsState()
                    CtrlButton(
                        isShift = isShift,
                        onInteraction = { overlayState.onInteraction() },
                        onKeyEvent = { down -> 
                            val key = if (isShift) "Shift" else "Control"
                            viewModel.sendKeyEvent(key, down) 
                        }
                    )
                }
            }
        }

        // Screen Flash and Thumbnail animation
        ScreenshotFlash(
            screenshotTriggerFlow = viewModel.screenshotTriggerFlow,
            screenshotSavedFlow = viewModel.screenshotSavedFlow
        )
    }

    // 詳細設定ダイアログ
    if (overlayState.showDetailedSettings) {
        val useOriginal by viewModel.useOriginalQualityScreenshot.collectAsState()
        val isShiftButtonEnabled by viewModel.isShiftButtonEnabled.collectAsState()
        val maxEdge by viewModel.maxEdge.collectAsState()
        DetailedSettingsDialog(
            useOriginalQualityScreenshot = useOriginal,
            isShiftButtonEnabled = isShiftButtonEnabled,
            maxEdge = maxEdge,
            onUseOriginalQualityScreenshotChange = { viewModel.setUseOriginalQualityScreenshot(it) },
            onShiftButtonEnabledChange = { viewModel.setShiftButtonEnabled(it) },
            onMaxEdgeChange = { viewModel.setMaxEdge(it) },
            onDismiss = { overlayState.showDetailedSettings = false }
        )
    }

    // 接続詳細情報ダイアログ
    if (overlayState.showConnectionDetails) {
        val selectedCodec by viewModel.selectedCodec.collectAsState()
        
        val sessionId = remember(signalingUrl) {
            try {
                val uri = android.net.Uri.parse(signalingUrl)
                uri.getQueryParameter("session_id") ?: "Unknown"
            } catch (e: Exception) {
                "Unknown"
            }
        }
        
        val iceConnectionState by viewModel.iceConnectionState.collectAsState()
        val signalingState by viewModel.signalingState.collectAsState()
        
        ConnectionDetailsDialog(
            rtcStats = rtcStats,
            deviceScreenSize = deviceScreenSize,
            connectionState = connectionState,
            webSocketState = webSocketState,
            iceConnectionState = iceConnectionState,
            signalingState = signalingState,
            selectedCodec = selectedCodec,
            sessionId = sessionId,
            onDismiss = { overlayState.showConnectionDetails = false }
        )
    }

    // CSV エクスポート確認ダイアログ
    if (showCsvExportDialog) {
        AlertDialog(
            onDismissRequest = { showCsvExportDialog = false },
            title = {
                Text(text = "Export Latency Data")
            },
            text = {
                Text("Do you want to export the accumulated latency data as a CSV file?")
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        showCsvExportDialog = false
                        val timestamp = java.text.SimpleDateFormat("yyyyMMdd_HHmmss", java.util.Locale.US).format(java.util.Date())
                        csvExportLauncher.launch("latency_$timestamp.csv")
                    }
                ) {
                    Text("Export")
                }
            },
            dismissButton = {
                TextButton(
                    onClick = { showCsvExportDialog = false }
                ) {
                    Text("Cancel")
                }
            }
        )
    }
}

/**
 * 接続フェーズの1行を表示するコンポーネント
 */
@Composable
private fun ConnectionPhaseRow(
    label: String,
    state: String
) {
    // 状態を色付きのインジケーターにマッピングする処理
    val (indicatorColor, displayState) = when {
        // Connected 状態
        state.equals("Connected", ignoreCase = true) || state.equals("COMPLETED", ignoreCase = true) ->
            Color(0xFF22C55E) to "Connected" // green-500
        
        // Error / Disconnected 状態
        state.startsWith("Error", ignoreCase = true) || 
        state.equals("Failed", ignoreCase = true) || 
        state.equals("Disconnected", ignoreCase = true) || 
        state.equals("CLOSED", ignoreCase = true) ||
        state.equals("FAILED", ignoreCase = true) ->
            Color(0xFFEF4444) to state // red-500
        
        // Initial 状態（new 等）
        state.equals("NEW", ignoreCase = true) || state.isEmpty() ->
            Color(0xFF52525B) to "Waiting..." // zinc-500
            
        // その他の処理中状態 (Connecting, CHECKING 等)
        else ->
            Color(0xFFEAB308) to state // yellow-500
    }

    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Start,
        modifier = Modifier.fillMaxWidth(0.6f) // 幅を制限して全体が中央寄りになるように
    ) {
        // インジケーター丸
        Box(
            modifier = Modifier
                .size(10.dp)
                .clip(CircleShape)
                .background(indicatorColor)
        )
        
        Spacer(modifier = Modifier.width(12.dp))
        
        // ラベル (例: "Signaling Server")
        Text(
            text = label,
            color = Color.White.copy(alpha = 0.9f),
            fontSize = 14.sp,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f)
        )
        
        // 状態テキスト (例: "Connected")
        Text(
            text = displayState,
            color = Color.White.copy(alpha = 0.6f),
            fontSize = 14.sp
        )
    }
}

/**
 * トップバー: 戻るボタン、ステータスバッジ、アクションボタン群
 */
@Composable
private fun TopBar(
    isConnected: Boolean,
    connectionState: String,
    rtcStats: WebRtcStats,
    isTrackpadModeEnabled: Boolean,
    onBack: () -> Unit,
    showSettings: Boolean,
    onToggleSettings: () -> Unit,
    onNavigateToGallery: () -> Unit,
    onToggleTrackpadMode: () -> Unit,
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
                val isError = connectionState == "Failed"
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
                                if (isConnected) Color(0xFF22C55E) else if (isError) Color(0xFFEF4444) else Color(0xFFEAB308)
                            )
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        text = if (isConnected) "connected" else if (isError) "error" else "connecting",
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
                // トラックパッド切替ボタン
                OverlayIconButton(
                    icon = if (isTrackpadModeEnabled) Icons.Default.Mouse else Icons.Default.TouchApp,
                    contentDescription = "入力モード切替",
                    onClick = onToggleTrackpadMode
                )
                // ギャラリーボタン
                OverlayIconButton(
                    icon = Icons.Default.Image,
                    contentDescription = "ギャラリー",
                    onClick = onNavigateToGallery
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
 * ゲーム上のオーバーレイボタン（押下状態の管理やイベントのバブリング防止などを行う共通コンポーネント）
 */
@Composable
private fun GameOverlayButton(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    contentDescription: String,
    isPressedColor: Color = Color(0xFF3B82F6).copy(alpha = 0.8f),
    normalColor: Color = Color(0xFF18181B).copy(alpha = 0.5f),
    onInteraction: () -> Unit,
    onPressedChange: ((Boolean) -> Unit)? = null,
    onClick: (() -> Unit)? = null
) {
    val haptic = androidx.compose.ui.platform.LocalHapticFeedback.current
    var isPressed by remember { mutableStateOf(false) }

    // 押下状態に伴うスケールアニメーション（へこむような効果）
    val scale by androidx.compose.animation.core.animateFloatAsState(
        targetValue = if (isPressed) 0.85f else 1.0f,
        animationSpec = androidx.compose.animation.core.spring(
            dampingRatio = androidx.compose.animation.core.Spring.DampingRatioMediumBouncy,
            stiffness = androidx.compose.animation.core.Spring.StiffnessLow
        ),
        label = "GameOverlayButtonScale"
    )

    // 押下状態に伴うカラーアニメーション
    val bgColor by animateColorAsState(
        targetValue = if (isPressed) isPressedColor else normalColor,
        label = "GameOverlayButtonBg"
    )
    val borderColor by animateColorAsState(
        targetValue = if (isPressed) Color(0xFF60A5FA) else Color.White.copy(alpha = 0.2f),
        label = "GameOverlayButtonBorder"
    )

    // 押下状態が切り替わったときにイベントを送信し、押下中は定期的にインタラクションを通知してUIが消えないようにする
    LaunchedEffect(isPressed) {
        onPressedChange?.invoke(isPressed)
        if (isPressed) {
            while (true) {
                onInteraction() // UIが自動非表示にならないよう定期的にタイマーリセット
                delay(1000)
            }
        } else {
            onInteraction() // 離した時も一度タイマーをリセット
        }
    }

    Box(
        modifier = Modifier
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
            }
            .size(64.dp)
            .clip(CircleShape)
            .background(bgColor)
            .border(width = if (isPressed) 2.dp else 1.dp, color = borderColor, shape = CircleShape)
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Main)
                        val down = event.changes.any { it.pressed }
                        if (down != isPressed) {
                            if (down) {
                                haptic.performHapticFeedback(androidx.compose.ui.hapticfeedback.HapticFeedbackType.LongPress)
                            } else {
                                // 離された時にonClickを発火
                                if (isPressed) {
                                    onClick?.invoke()
                                }
                            }
                            isPressed = down
                        }
                        // タップイベントが下の要素へ伝播してクリック判定になるのを防ぐ
                        event.changes.forEach { it.consume() }
                    }
                }
            },
        contentAlignment = Alignment.Center
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            Icon(
                imageVector = icon,
                contentDescription = contentDescription,
                tint = if (isPressed) Color.White else Color.White.copy(alpha = 0.8f),
                modifier = Modifier.size(24.dp)
            )
            Text(
                text = label,
                color = if (isPressed) Color.White else Color.White.copy(alpha = 0.8f),
                fontWeight = FontWeight.ExtraBold,
                fontSize = 10.sp,
                letterSpacing = 1.sp
            )
        }
    }
}

/**
 * Ctrlキー送信用ボタン
 */
@Composable
private fun CtrlButton(
    isShift: Boolean = false,
    onInteraction: () -> Unit,
    onKeyEvent: (Boolean) -> Unit
) {
    GameOverlayButton(
        icon = Icons.Default.Keyboard,
        label = if (isShift) "SHIFT" else "CTRL",
        contentDescription = if (isShift) "Shift Key" else "Ctrl Key",
        onInteraction = onInteraction,
        onPressedChange = onKeyEvent
    )
}

/**
 * スクリーンショット用ボタン
 */
@Composable
private fun ScreenshotButton(
    onInteraction: () -> Unit,
    onClick: () -> Unit
) {
    GameOverlayButton(
        icon = Icons.Default.CameraAlt,
        label = "CAP",
        contentDescription = "スクリーンショット",
        onInteraction = onInteraction,
        onClick = onClick
    )
}

private const val STATS_GRAPH_WINDOW_MS = 30_000L

private data class StatsGraphPoint(
    val timestampMs: Long,
    val rttMs: Int,
    val clockOffsetMs: Int,
    val latencyMs: Int
)

/**
 * デバッグパネル: 頻繁に変わる統計情報を表示 (FPS/Bitrate/Loss)
 */
@Composable
private fun DebugPanel(
    rtcStats: WebRtcStats,
    latestLatencySample: moe.ryoha.remoterg.webrtc.LatencySample?,
    deviceScreenSize: String
) {
    Column(
        modifier = Modifier
            .width(220.dp) // 幅を広げてラベルを収める
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
            Text(text = "Loss", style = monoStyle, modifier = Modifier.width(84.dp))
            Text(text = "${rtcStats.loss}%", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
        }
        Row(modifier = Modifier.fillMaxWidth()) {
            Text(text = "E2E Latency", style = monoStyle, modifier = Modifier.width(84.dp))
            Text(text = "${rtcStats.latencyMs} ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
        }
        
        if (latestLatencySample != null) {
            val netAndOther = (rtcStats.latencyMs - latestLatencySample.captureToEncInMs - latestLatencySample.encInToEncOutMs - latestLatencySample.encOutToSendMs - rtcStats.jitterBufferMs - rtcStats.decodeTimeMs).coerceAtLeast(0)
            
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(text = "├ Capture", style = monoStyle, modifier = Modifier.width(84.dp))
                Text(text = "${latestLatencySample.captureToEncInMs} ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
            }
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(text = "├ Encode", style = monoStyle, modifier = Modifier.width(84.dp))
                Text(text = "${latestLatencySample.encInToEncOutMs} ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
            }
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(text = "├ Host Q", style = monoStyle, modifier = Modifier.width(84.dp))
                Text(text = "${latestLatencySample.encOutToSendMs} ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
            }
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(text = "├ Net+Other", style = monoStyle, modifier = Modifier.width(84.dp))
                Text(text = "$netAndOther ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
            }
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(text = "├ JitterBuf", style = monoStyle, modifier = Modifier.width(84.dp))
                Text(text = "${rtcStats.jitterBufferMs} ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
            }
            Row(modifier = Modifier.fillMaxWidth()) {
                Text(text = "└ Decode", style = monoStyle, modifier = Modifier.width(84.dp))
                Text(text = "${rtcStats.decodeTimeMs} ms", style = monoStyle, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.End)
            }
        }
    }
}

@Composable
private fun StatsGraphPanel(
    rtcStats: WebRtcStats,
    history: List<StatsGraphPoint>,
    onClick: () -> Unit
) {
    Column(
        modifier = Modifier
            .width(200.dp)
            .background(
                Color.Black.copy(alpha = 0.6f),
                RoundedCornerShape(8.dp)
            )
            .border(
                1.dp,
                Color.White.copy(alpha = 0.1f),
                RoundedCornerShape(8.dp)
            )
            .clickable { onClick() }
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        Text(
            text = "Stats Graph (30s)",
            color = Color(0xFFE4E4E7),
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium
        )

        MetricSparkline(
            label = "RTT",
            valueText = "${rtcStats.rttMs} ms",
            color = Color(0xFF60A5FA),
            history = history,
            valueSelector = { it.rttMs }
        )
        MetricSparkline(
            label = "Offset",
            valueText = "${rtcStats.clockOffsetMs} ms",
            color = Color(0xFFFBBF24),
            history = history,
            valueSelector = { it.clockOffsetMs }
        )
        MetricSparkline(
            label = "Latency",
            valueText = "${rtcStats.latencyMs} ms",
            color = Color(0xFF4ADE80),
            history = history,
            valueSelector = { it.latencyMs }
        )
    }
}

@Composable
private fun MetricSparkline(
    label: String,
    valueText: String,
    color: Color,
    history: List<StatsGraphPoint>,
    valueSelector: (StatsGraphPoint) -> Int
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = label,
                color = color,
                fontSize = 11.sp,
                fontWeight = FontWeight.Medium
            )
            Text(
                text = valueText,
                color = Color(0xFFE4E4E7),
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace
            )
        }

        Canvas(
            modifier = Modifier
                .fillMaxWidth()
                .height(36.dp)
                .background(Color.White.copy(alpha = 0.05f), RoundedCornerShape(6.dp))
        ) {
            if (history.isEmpty()) return@Canvas

            val windowStart = SystemClock.elapsedRealtime() - STATS_GRAPH_WINDOW_MS
            val minValue = history.minOf { valueSelector(it) }
            val maxValue = history.maxOf { valueSelector(it) }
            val range = (maxValue - minValue).coerceAtLeast(1)

            val path = Path()
            history.forEachIndexed { index, point ->
                val x = ((point.timestampMs - windowStart).toFloat() / STATS_GRAPH_WINDOW_MS.toFloat())
                    .coerceIn(0f, 1f) * size.width
                val y = size.height - (((valueSelector(point) - minValue).toFloat() / range.toFloat())
                    .coerceIn(0f, 1f) * size.height)
                if (index == 0) path.moveTo(x, y) else path.lineTo(x, y)
            }

            drawPath(
                path = path,
                color = color,
                style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round)
            )
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
    webSocketState: String,
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
                "WebSocket" to webSocketState,
                "WebRTC" to signalingState,
                "ICE" to iceConnectionState,
                "Codec" to selectedCodec,
                "Device" to deviceScreenSize,
                "Stream" to "${rtcStats.frameWidth}x${rtcStats.frameHeight}",
                "E2E Latency" to "${rtcStats.latencyMs} ms",
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
    onShowDetailedSettings: () -> Unit,
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
            modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
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

        Button(
            onClick = onShowDetailedSettings,
            modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp),
            shape = RoundedCornerShape(8.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = Color(0xFF3F3F46) // zinc-700
            )
        ) {
            Text(
                text = "詳細設定を開く",
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

/**
 * 詳細設定ダイアログ
 */
@Composable
private fun DetailedSettingsDialog(
    useOriginalQualityScreenshot: Boolean,
    isShiftButtonEnabled: Boolean,
    maxEdge: Int,
    onUseOriginalQualityScreenshotChange: (Boolean) -> Unit,
    onShiftButtonEnabledChange: (Boolean) -> Unit,
    onMaxEdgeChange: (Int) -> Unit,
    onDismiss: () -> Unit
) {
    androidx.compose.ui.window.Dialog(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .width(320.dp)
                .background(Color(0xFF18181B), RoundedCornerShape(12.dp))
                .border(1.dp, Color(0xFF27272A), RoundedCornerShape(12.dp))
                .padding(16.dp)
        ) {
            Text(
                text = "詳細設定",
                color = Color.White,
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp,
                modifier = Modifier.padding(bottom = 16.dp)
            )

            // スクリーンショット設定
            Column(modifier = Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "オリジナル画質で保存",
                        color = Color.White,
                        fontSize = 14.sp
                    )
                    Switch(
                        checked = useOriginalQualityScreenshot,
                        onCheckedChange = onUseOriginalQualityScreenshotChange,
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = Color.White,
                            checkedTrackColor = Color(0xFF3B82F6),
                            uncheckedThumbColor = Color(0xFFA1A1AA),
                            uncheckedTrackColor = Color(0xFF3F3F46),
                            uncheckedBorderColor = Color.Transparent
                        )
                    )
                }

                Spacer(modifier = Modifier.height(8.dp))

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.Top
                ) {
                    Icon(
                        imageVector = Icons.Default.ErrorOutline,
                        contentDescription = "Info",
                        tint = Color(0xFF60A5FA), // blue-400
                        modifier = Modifier.size(16.dp).padding(top = 2.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        text = if (useOriginalQualityScreenshot) {
                            "Host PC側からオリジナル画質のスクリーンショットを取得します。画質は良いですが、取得まで時間がかかる場合があります。"
                        } else {
                            "現在画面に表示されている映像を即時に保存します。画質は少し劣りますが、瞬時に保存できます。"
                        },
                        color = Color(0xFFA1A1AA), // zinc-400
                        fontSize = 12.sp,
                        lineHeight = 16.sp
                    )
                }
            }

            Spacer(modifier = Modifier.height(24.dp))

            // キー割り当て設定
            Column(modifier = Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "修飾キーの切り替え (Ctrl / Shift)",
                        color = Color.White,
                        fontSize = 14.sp
                    )
                    Switch(
                        checked = isShiftButtonEnabled,
                        onCheckedChange = onShiftButtonEnabledChange,
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = Color.White,
                            checkedTrackColor = Color(0xFF3B82F6),
                            uncheckedThumbColor = Color(0xFFA1A1AA),
                            uncheckedTrackColor = Color(0xFF3F3F46),
                            uncheckedBorderColor = Color.Transparent
                        )
                    )
                }

                Spacer(modifier = Modifier.height(8.dp))

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.Top
                ) {
                    Icon(
                        imageVector = Icons.Default.ErrorOutline,
                        contentDescription = "Info",
                        tint = Color(0xFF60A5FA), // blue-400
                        modifier = Modifier.size(16.dp).padding(top = 2.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        text = "オンにすると、画面右下のボタンがShiftキーとして動作します。",
                        color = Color(0xFFA1A1AA), // zinc-400
                        fontSize = 12.sp,
                        lineHeight = 16.sp
                    )
                }
            }

            Spacer(modifier = Modifier.height(24.dp))

            // 送信画像サイズ設定
            Column(modifier = Modifier.fillMaxWidth()) {
                var expanded by remember { mutableStateOf(false) }
                val options = listOf(
                    512 to "512px (高速)",
                    1024 to "1024px (標準)",
                    2048 to "2048px (高画質)",
                    99999 to "縮小しない"
                )
                val selectedOptionText = options.find { it.first == maxEdge }?.second ?: "${maxEdge}px"

                Text(
                    text = "送信画像サイズ (max_edge)",
                    color = Color.White,
                    fontSize = 14.sp,
                    modifier = Modifier.padding(bottom = 8.dp)
                )

                Box(modifier = Modifier.fillMaxWidth()) {
                    OutlinedButton(
                        onClick = { expanded = true },
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = Color.White
                        ),
                        border = BorderStroke(1.dp, Color(0xFF3F3F46)),
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(text = selectedOptionText)
                            Text(
                                text = if (expanded) "▲" else "▼",
                                color = Color.White
                            )
                        }
                    }

                    DropdownMenu(
                        expanded = expanded,
                        onDismissRequest = { expanded = false },
                        modifier = Modifier
                            .background(Color(0xFF18181B))
                            .border(1.dp, Color(0xFF27272A), RoundedCornerShape(4.dp))
                    ) {
                        options.forEach { option ->
                            DropdownMenuItem(
                                text = { Text(option.second, color = Color.White) },
                                onClick = {
                                    onMaxEdgeChange(option.first)
                                    expanded = false
                                }
                            )
                        }
                    }
                }

                Spacer(modifier = Modifier.height(8.dp))

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.Top
                ) {
                    Icon(
                        imageVector = Icons.Default.ErrorOutline,
                        contentDescription = "Info",
                        tint = Color(0xFF60A5FA), // blue-400
                        modifier = Modifier.size(16.dp).padding(top = 2.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        text = "スクリーンショットやAI解析時に、Host PCで画像を縮小してから転送します。\n「縮小しない」は通信量とPC負荷が非常に大きくなります。",
                        color = Color(0xFFA1A1AA), // zinc-400
                        fontSize = 12.sp,
                        lineHeight = 16.sp
                    )
                }
            }

            Spacer(modifier = Modifier.height(24.dp))

            Button(
                onClick = onDismiss,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF3F3F46))
            ) {
                Text("閉じる", color = Color.White)
            }
        }
    }
}
