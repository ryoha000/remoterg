package moe.ryoha.remoterg.ui.screens

import androidx.compose.animation.*
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.outlined.DesktopWindows
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.LocationOn
import androidx.compose.material.icons.outlined.ChatBubbleOutline
import androidx.compose.material.icons.outlined.PeopleOutline
import androidx.compose.material.icons.outlined.CalendarToday
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import coil.compose.AsyncImage
import kotlinx.coroutines.launch
import moe.ryoha.remoterg.ui.theme.*
import moe.ryoha.remoterg.ui.viewmodel.GalleryViewModel
import java.text.SimpleDateFormat
import java.util.*
import kotlin.math.abs
import kotlin.math.roundToInt

private const val INFO_PANEL_WIDTH_PCT = 0.35f
// スワイプで戻る閾値（RN版: translationY > 100 || velocityY > 500）
private const val DISMISS_THRESHOLD_PX = 100f
private const val DISMISS_VELOCITY_THRESHOLD = 500f

@OptIn(ExperimentalSharedTransitionApi::class, ExperimentalFoundationApi::class)
@Composable
fun GalleryDetailScreen(
    initialLocalId: String,
    onNavigateBack: () -> Unit,
    sharedTransitionScope: SharedTransitionScope,
    animatedVisibilityScope: AnimatedVisibilityScope,
    viewModel: GalleryViewModel = hiltViewModel()
) {
    val screenshots by viewModel.screenshots.collectAsState()
    val favorites by viewModel.favorites.collectAsState()
    val analysisResults by viewModel.analysisResults.collectAsState()
    val isAnalyzingMap by viewModel.isAnalyzingMap.collectAsState()
    val isConnected by viewModel.isConnected.collectAsState()

    if (screenshots.isEmpty()) return

    // 一覧画面と同じ並び順を使用（降順）
    val initialIndex = remember(screenshots, initialLocalId) {
        screenshots.indexOfFirst { it.localId == initialLocalId }.takeIf { it >= 0 } ?: 0
    }

    val pagerState = rememberPagerState(initialPage = initialIndex) { screenshots.size }
    
    val currentScreenshot = screenshots.getOrNull(pagerState.currentPage)
    
    LaunchedEffect(currentScreenshot) {
        currentScreenshot?.let { ss ->
            if (!analysisResults.containsKey(ss.localId)) {
                viewModel.loadAnalysisResult(ss.localId)
            }
        }
    }
    val coroutineScope = rememberCoroutineScope()

    // InfoPanel 表示状態（RN版の toggleInfo に相当）
    var showInfo by remember { mutableStateOf(false) }

    // InfoPanel 連動アニメーション（RN版の infoOpenAnim に相当: 0f=閉, 1f=開）
    val infoAnimFloat = remember { Animatable(0f) }
    LaunchedEffect(showInfo) {
        infoAnimFloat.animateTo(
            targetValue = if (showInfo) 1f else 0f,
            animationSpec = tween(300, easing = FastOutSlowInEasing)
        )
    }

    // 上下ドラッグ状態（RN版の translationY に相当）
    val offsetY = remember { Animatable(0f) }

    val isFavorite = currentScreenshot?.let { ss -> favorites.any { it.localId == ss.localId } } ?: false
    val analysisResult = currentScreenshot?.let { ss -> analysisResults[ss.localId] }
    val isAnalyzing = currentScreenshot?.let { ss -> isAnalyzingMap[ss.hostId] } ?: false

    val dateFormat = remember { 
        SimpleDateFormat("yyyy/MM/dd HH:mm:ss", Locale.US).apply {
            timeZone = TimeZone.getTimeZone("Asia/Tokyo")
        }
    }
    val dateString = currentScreenshot?.let { dateFormat.format(Date(it.dateAdded * 1000L)) } ?: ""

    // InfoPanel の幅（画面幅の 35%）
    val screenWidthDp = LocalConfiguration.current.screenWidthDp.dp
    val infoPanelWidth = screenWidthDp * INFO_PANEL_WIDTH_PCT

    // 画面高さ（ドラッグスケール計算用）
    val density = LocalDensity.current
    val screenHeightPx = with(density) { LocalConfiguration.current.screenHeightDp.dp.toPx() }

    val context = LocalContext.current

    // 上ボーダー描画用のピクセル値
    val borderWidthPx = with(density) { 1.dp.toPx() }

    // ドラッグ中のスケール（RN版: interpolate(abs(translationY), [0, screenHeight], [1, 0.8])）
    val dragScale = (1f - (abs(offsetY.value) / screenHeightPx) * 0.2f).coerceIn(0.8f, 1f)

    // ドラッグ中の背景透明度（RN版: backdropStyle の opacity）
    val bgAlpha = (1f - abs(offsetY.value) / (screenHeightPx * 0.5f)).coerceIn(0f, 1f)

    // 画像エリアの幅割合（InfoPanel 開閉に連動）
    // RN版: interpolate(infoOpenAnim, [0,1], [screenWidth, screenWidth - infoPanelWidth])
    val imageAreaFraction = 1f - (INFO_PANEL_WIDTH_PCT * infoAnimFloat.value)

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = bgAlpha))
    ) {
        // カルーセル（画像エリア）
        HorizontalPager(
            state = pagerState,
            modifier = Modifier
                .fillMaxHeight()
                .fillMaxWidth(fraction = imageAreaFraction)
                .offset { IntOffset(0, offsetY.value.roundToInt()) }
                .graphicsLayer {
                    scaleX = dragScale
                    scaleY = dragScale
                }
                .pointerInput(Unit) {
                    // 上下スワイプで戻るジェスチャー（RN版の panGesture に相当）
                    detectVerticalDragGestures(
                        onDragEnd = {
                            coroutineScope.launch {
                                if (abs(offsetY.value) > DISMISS_THRESHOLD_PX) {
                                    // 閾値を超えた → 一覧に戻る
                                    onNavigateBack()
                                } else {
                                    // 閾値未満 → 元位置にスプリングで戻す
                                    offsetY.animateTo(
                                        0f,
                                        animationSpec = SpringSpec(
                                            dampingRatio = Spring.DampingRatioMediumBouncy,
                                            stiffness = Spring.StiffnessMedium
                                        )
                                    )
                                }
                            }
                        },
                        onVerticalDrag = { _, dragAmount ->
                            coroutineScope.launch {
                                offsetY.snapTo(offsetY.value + dragAmount)
                            }
                        }
                    )
                }
        ) { page ->
            val screenshot = screenshots[page]

            with(sharedTransitionScope) {
                AsyncImage(
                    model = screenshot.uri,
                    contentDescription = null,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier
                        .fillMaxSize()
                        .sharedElement(
                            state = rememberSharedContentState(key = "image-${screenshot.localId}"),
                            animatedVisibilityScope = animatedVisibilityScope
                        )
                        .pointerInput(Unit) {
                            // タップで InfoPanel 表示切替（RN版の tapGesture に相当）
                            detectTapGestures(
                                onTap = { showInfo = !showInfo }
                            )
                        }
                )
            }
        }

        // --- オーバーレイ ---

        // ヘッダー（RN版 ScreenshotDetailHeader.tsx に準拠）
        // RN: overlayControlsStyle で right = infoPanelWidth * infoAnim
        AnimatedVisibility(
            visible = showInfo,
            enter = fadeIn(animationSpec = tween(200)),
            exit = fadeOut(animationSpec = tween(200)),
            modifier = Modifier
                .align(Alignment.TopStart)
                .fillMaxWidth(fraction = imageAreaFraction)
        ) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color.Black.copy(alpha = 0.4f))
                    .statusBarsPadding()
                    .padding(vertical = 2.dp)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 4.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    // 戻るボタン（RN: rounded-full bg-black/20）
                    IconButton(
                        onClick = onNavigateBack,
                        modifier = Modifier
                            .clip(CircleShape)
                            .background(Color.Black.copy(alpha = 0.2f))
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "戻る",
                            tint = Color.White
                        )
                    }

                    Spacer(modifier = Modifier.weight(1f))

                    // お気に入りボタン（RN: heart / heart-outline, #f91980）
                    IconButton(
                        onClick = {
                            currentScreenshot?.let {
                                viewModel.toggleFavorite(it.localId, isFavorite)
                            }
                        }
                    ) {
                        Icon(
                            imageVector = if (isFavorite) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                            contentDescription = "お気に入り",
                            tint = if (isFavorite) Color(0xFFF91980) else Color.White
                        )
                    }
                }
            }
        }

        // アクションバー（RN版 ScreenshotDetailActions.tsx に準拠）
        // RN: overlayControlsStyle で right = infoPanelWidth * infoAnim
        AnimatedVisibility(
            visible = showInfo,
            enter = fadeIn(animationSpec = tween(200)) + slideInVertically(animationSpec = tween(200)) { it },
            exit = fadeOut(animationSpec = tween(200)) + slideOutVertically(animationSpec = tween(200)) { it },
            modifier = Modifier
                .align(Alignment.BottomStart)
                .fillMaxWidth(fraction = imageAreaFraction)
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .drawBehind {
                        // 上ボーダー (white/10) — RN: border-t border-white/10
                        drawRect(
                            color = Color.White.copy(alpha = 0.1f),
                            topLeft = Offset.Zero,
                            size = Size(size.width, borderWidthPx)
                        )
                    }
                    .background(Color.Black.copy(alpha = 0.6f))
                    .navigationBarsPadding()
                    .padding(top = 16.dp, bottom = 32.dp),
                horizontalArrangement = Arrangement.SpaceAround,
                verticalAlignment = Alignment.CenterVertically
            ) {
                // Twitter ボタン
                ActionButton(
                    icon = {
                        Text("X", color = Color.White, fontWeight = FontWeight.Bold, fontSize = 18.sp)
                    },
                    label = "Twitter",
                    labelColor = Color.White,
                    onClick = {
                        currentScreenshot?.let { ss ->
                            val tweetIntent = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                                putExtra(android.content.Intent.EXTRA_STREAM, ss.uri)
                                type = "image/*"
                                `package` = "com.twitter.android"
                            }
                            try {
                                context.startActivity(tweetIntent)
                            } catch (e: android.content.ActivityNotFoundException) {
                                android.widget.Toast.makeText(context, "X/Twitterアプリがインストールされていません", android.widget.Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                )

                // 共有ボタン
                ActionButton(
                    icon = {
                        Icon(
                            imageVector = Icons.Default.Share,
                            contentDescription = "共有",
                            tint = Color.White,
                            modifier = Modifier.size(20.dp)
                        )
                    },
                    label = "共有",
                    labelColor = Color.White,
                    onClick = {
                        currentScreenshot?.let { ss ->
                            val shareIntent = android.content.Intent().apply {
                                action = android.content.Intent.ACTION_SEND
                                putExtra(android.content.Intent.EXTRA_STREAM, ss.uri)
                                type = "image/*"
                            }
                            context.startActivity(android.content.Intent.createChooser(shareIntent, "画像を共有"))
                        }
                    }
                )

                // ゴミ箱ボタン（RN版ラベル: "ゴミ箱"）
                ActionButton(
                    icon = {
                        Icon(
                            imageVector = Icons.Default.Delete,
                            contentDescription = "ゴミ箱",
                            tint = Color.White,
                            modifier = Modifier.size(20.dp)
                        )
                    },
                    label = "ゴミ箱",
                    labelColor = Color.White,
                    onClick = {
                        currentScreenshot?.let {
                            viewModel.deleteScreenshot(it.localId)
                            onNavigateBack()
                        }
                    }
                )
            }
        }

        // InfoPanel（RN版 ScreenshotDetailInfoPanel.tsx に準拠）
        // 右からスライドインするパネル
        AnimatedVisibility(
            visible = showInfo,
            enter = slideInHorizontally(animationSpec = tween(300)) { it } + fadeIn(animationSpec = tween(300)),
            exit = slideOutHorizontally(animationSpec = tween(300)) { it } + fadeOut(animationSpec = tween(300)),
            modifier = Modifier
                .align(Alignment.CenterEnd)
                .fillMaxHeight()
                .width(infoPanelWidth)
        ) {
            InfoPanel(
                dateString = dateString,
                screenshot = currentScreenshot,
                analysisResult = analysisResult,
                isAnalyzing = isAnalyzing,
                isConnected = isConnected,
                onRequestAnalyze = { hostId -> viewModel.requestAnalyze(hostId, 512) },
                modifier = Modifier.fillMaxSize()
            )
        }
    }
}

/**
 * アクションバー内の個別ボタン
 */
@Composable
private fun ActionButton(
    icon: @Composable () -> Unit,
    label: String,
    labelColor: Color,
    onClick: () -> Unit
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.padding(horizontal = 8.dp)
    ) {
        IconButton(onClick = onClick) {
            icon()
        }
        Text(
            text = label,
            color = labelColor,
            fontSize = 12.sp
        )
    }
}

/**
 * 右パネル: スクリーンショット情報パネル
 * RN版 ScreenshotDetailInfoPanel.tsx に準拠
 * - zinc-900 背景, zinc-800 左ボーダー
 * - 日時 / 詳細 / アプリケーション / AI Analysis セクション
 */
@Composable
private fun InfoPanel(
    dateString: String,
    screenshot: moe.ryoha.remoterg.data.repository.MediaStoreScreenshot?,
    analysisResult: moe.ryoha.remoterg.data.model.AnalysisResult?,
    isAnalyzing: Boolean,
    isConnected: Boolean,
    onRequestAnalyze: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val density = LocalDensity.current
    val borderPx = with(density) { 1.dp.toPx() }

    Column(
        modifier = modifier
            // RN: backgroundColor: "#18181b" (zinc-900), borderLeftWidth: 1, borderColor: "#27272a" (zinc-800)
            .drawBehind {
                // 左ボーダー
                drawRect(
                    color = Zinc800,
                    topLeft = Offset.Zero,
                    size = Size(borderPx, size.height)
                )
            }
            .background(Zinc900)
            .statusBarsPadding()
            .verticalScroll(rememberScrollState())
            .padding(16.dp)
    ) {
        // 日時セクション（RN: calendar-outline + "日時"）
        InfoSectionHeader(
            icon = {
                Icon(
                    imageVector = Icons.Outlined.CalendarToday,
                    contentDescription = null,
                    tint = Zinc400,
                    modifier = Modifier.size(16.dp)
                )
            },
            title = "日時"
        )
        Text(
            text = dateString,
            color = Zinc200,
            fontSize = 16.sp,
            modifier = Modifier.padding(bottom = 24.dp)
        )

        // 詳細セクション（RN: image-outline + "詳細"）
        InfoSectionHeader(
            icon = {
                Icon(
                    imageVector = Icons.Outlined.Image,
                    contentDescription = null,
                    tint = Zinc400,
                    modifier = Modifier.size(16.dp)
                )
            },
            title = "詳細"
        )
        screenshot?.let { ss ->
            Text(
                text = "${ss.width} x ${ss.height}",
                color = Zinc200,
                fontSize = 16.sp
            )
            Text(
                text = ss.localId,
                color = Zinc400,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 4.dp, bottom = 24.dp)
            )
        }

        // アプリケーションセクション（RN: desktop-outline + "アプリケーション"）
        screenshot?.let { ss ->
            if (ss.windowTitle.isNotBlank() || ss.processName.isNotBlank()) {
                InfoSectionHeader(
                    icon = {
                        Icon(
                            imageVector = Icons.Outlined.DesktopWindows,
                            contentDescription = null,
                            tint = Zinc400,
                            modifier = Modifier.size(16.dp)
                        )
                    },
                    title = "アプリケーション"
                )
                if (ss.windowTitle.isNotBlank()) {
                    Text(
                        text = ss.windowTitle,
                        color = Zinc200,
                        fontSize = 16.sp
                    )
                }
                if (ss.processName.isNotBlank()) {
                    Text(
                        text = ss.processName,
                        color = Zinc400,
                        fontSize = 14.sp,
                        modifier = Modifier.padding(top = 4.dp)
                    )
                }
                Spacer(modifier = Modifier.height(24.dp))
            }
        }

        // AI Analysis セクション
        Spacer(modifier = Modifier.height(8.dp))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(bottom = 16.dp)
        ) {
            Icon(
                imageVector = Icons.Outlined.AutoAwesome,
                contentDescription = null,
                tint = Color(0xFFA855F7), // purple-500
                modifier = Modifier.size(16.dp)
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = "AI Analysis",
                color = Color.White,
                fontSize = 16.sp,
                fontWeight = FontWeight.Bold
            )
        }

        if (analysisResult != null) {
            AnalysisViewer(analysisResult)
            if (isAnalyzing) {
                // Update中の表示
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(8.dp),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(16.dp),
                        color = Color(0xFFA855F7),
                        strokeWidth = 2.dp
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(text = "Updating...", color = Zinc400, fontSize = 12.sp)
                }
            }
        } else if (isAnalyzing) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 32.dp),
                contentAlignment = Alignment.Center
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator(color = Color(0xFFA855F7))
                    Spacer(modifier = Modifier.height(12.dp))
                    Text(text = "Analyzing image context...", color = Zinc500, fontSize = 14.sp)
                }
            }
        } else {
            // 未解析状態
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(8.dp))
                    .border(1.dp, Zinc800, RoundedCornerShape(8.dp))
                    .background(Zinc900)
                    .padding(16.dp),
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Text(
                        text = "Get insights about the scene, characters, and dialogue using AI.",
                        color = Zinc400,
                        fontSize = 14.sp,
                        lineHeight = 20.sp,
                        modifier = Modifier.fillMaxWidth()
                    )
                    OutlinedButton(
                        onClick = { screenshot?.let { onRequestAnalyze(it.hostId) } },
                        enabled = isConnected && screenshot != null,
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.outlinedButtonColors(
                            containerColor = Zinc800,
                            contentColor = Zinc200,
                            disabledContainerColor = Zinc800.copy(alpha = 0.5f),
                            disabledContentColor = Zinc500
                        ),
                        border = BorderStroke(1.dp, if (isConnected) Zinc700 else Zinc800)
                    ) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.AutoAwesome,
                                contentDescription = null,
                                tint = if (isConnected) Color(0xFFC084FC) else Zinc500,
                                modifier = Modifier.size(16.dp)
                            )
                            Text(
                                text = if (isConnected) "Analyze Screenshot" else "Offline",
                                color = if (isConnected) Zinc200 else Zinc500
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * InfoPanel 内のセクションヘッダー
 */
@Composable
private fun InfoSectionHeader(
    icon: @Composable () -> Unit,
    title: String
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.padding(bottom = 8.dp)
    ) {
        icon()
        Spacer(modifier = Modifier.width(8.dp))
        Text(
            text = title,
            color = Zinc400,
            fontWeight = FontWeight.Medium,
            fontSize = 14.sp
        )
    }
}

/**
 * 解析結果の表示ビュー
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AnalysisViewer(analysis: moe.ryoha.remoterg.data.model.AnalysisResult) {
    Column(verticalArrangement = Arrangement.spacedBy(24.dp)) {
        
        // Scene Info
        analysis.sceneInfo?.let { scene ->
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        imageVector = Icons.Outlined.LocationOn,
                        contentDescription = null,
                        tint = Zinc400,
                        modifier = Modifier.size(14.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(text = "Scene", color = Zinc400, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                }
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(6.dp))
                        .background(Zinc900.copy(alpha = 0.5f))
                        .border(1.dp, Zinc800, RoundedCornerShape(6.dp))
                        .padding(12.dp)
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Row {
                            Text(text = "Location", color = Zinc500, fontSize = 14.sp, modifier = Modifier.width(80.dp))
                            Text(text = scene.location, color = Zinc200, fontSize = 14.sp, modifier = Modifier.weight(1f))
                        }
                        Row {
                            Text(text = "Time", color = Zinc500, fontSize = 14.sp, modifier = Modifier.width(80.dp))
                            Text(text = scene.timeOfDay, color = Zinc200, fontSize = 14.sp, modifier = Modifier.weight(1f))
                        }
                        Row {
                            Text(text = "Mood", color = Zinc500, fontSize = 14.sp, modifier = Modifier.width(80.dp))
                            Text(text = scene.atmosphere, color = Zinc200, fontSize = 14.sp, modifier = Modifier.weight(1f))
                        }
                    }
                }
            }
        }

        // Dialogue
        analysis.dialogue?.let { dialog ->
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        imageVector = Icons.Outlined.ChatBubbleOutline,
                        contentDescription = null,
                        tint = Zinc400,
                        modifier = Modifier.size(14.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(text = "Dialogue", color = Zinc400, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                }
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(6.dp))
                        .background(Zinc900.copy(alpha = 0.5f))
                        .border(1.dp, Zinc800, RoundedCornerShape(6.dp))
                        .padding(12.dp)
                ) {
                    Column {
                        Text(text = dialog.speaker, color = Color(0xFFA5B4FC), fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(bottom = 4.dp)) // indigo-300
                        Text(text = dialog.text, color = Zinc200, lineHeight = 22.sp)
                    }
                }
            }
        }

        // Characters
        if (analysis.characters.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        imageVector = Icons.Outlined.PeopleOutline,
                        contentDescription = null,
                        tint = Zinc400,
                        modifier = Modifier.size(14.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(text = "Characters", color = Zinc400, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                }
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    analysis.characters.forEach { char ->
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(6.dp))
                                .background(Zinc900.copy(alpha = 0.5f))
                                .border(1.dp, Zinc800, RoundedCornerShape(6.dp))
                                .padding(12.dp)
                        ) {
                            Column {
                                Row(
                                    modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Text(text = char.name, color = Color(0xFF6EE7B7), fontWeight = FontWeight.SemiBold) // emerald-300
                                    Text(text = char.position.uppercase(), color = Zinc500, fontSize = 12.sp)
                                }
                                Text(
                                    text = char.visualDescription,
                                    color = Zinc300,
                                    fontSize = 14.sp,
                                    modifier = Modifier.padding(bottom = 8.dp)
                                )
                                FlowRow(
                                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                                    verticalArrangement = Arrangement.spacedBy(4.dp)
                                ) {
                                    char.expressionTags.forEach { tag ->
                                        Box(
                                            modifier = Modifier
                                                .clip(RoundedCornerShape(4.dp))
                                                .background(Zinc800)
                                                .padding(horizontal = 6.dp, vertical = 2.dp)
                                        ) {
                                            Text(text = tag, color = Zinc400, fontSize = 10.sp)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
