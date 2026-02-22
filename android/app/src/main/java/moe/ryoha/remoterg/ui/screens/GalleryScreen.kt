package moe.ryoha.remoterg.ui.screens

import android.net.Uri
import android.provider.MediaStore
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.ImageNotSupported
import androidx.compose.material3.*
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import coil.compose.AsyncImage
import moe.ryoha.remoterg.data.repository.MediaStoreScreenshot
import moe.ryoha.remoterg.ui.viewmodel.GalleryViewModel
import coil.request.ImageRequest
import coil.size.Size
import androidx.compose.ui.platform.LocalContext
import java.io.File
import kotlin.math.roundToInt
import moe.ryoha.remoterg.ui.viewmodel.DateSection
import moe.ryoha.remoterg.ui.viewmodel.JustifiedItem
import moe.ryoha.remoterg.ui.viewmodel.JustifiedRow

private val TARGET_ROW_HEIGHT = 180.dp
private val SPACING = 4.dp

@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
fun GalleryScreen(
    onNavigateBack: () -> Unit,
    onNavigateToDetail: (String) -> Unit,
    sharedTransitionScope: SharedTransitionScope,
    animatedVisibilityScope: AnimatedVisibilityScope,
    viewModel: GalleryViewModel = hiltViewModel()
) {
    val screenshots by viewModel.screenshots.collectAsState()
    val searchFilters by viewModel.searchFilters.collectAsState()
    val recentTitles by viewModel.recentTitles.collectAsState()

    val screenWidthDp = LocalConfiguration.current.screenWidthDp.toFloat()
    val density = LocalDensity.current

    // サムネイルのピクセルサイズを事前計算（Coil のダウンサンプリング用）
    val thumbnailHeightPx = remember(density) {
        with(density) { TARGET_ROW_HEIGHT.toPx().roundToInt() }
    }

    val sections by viewModel.sections.collectAsState()

    androidx.compose.runtime.LaunchedEffect(screenWidthDp) {
        viewModel.updateScreenWidth(screenWidthDp)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding()
    ) {
        // Custom App Bar with Back Button and Search Panel
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 4.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onNavigateBack) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "Back",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Spacer(modifier = Modifier.width(4.dp))
            SearchPanel(
                filters = searchFilters,
                onFiltersChanged = viewModel::updateFilters,
                recentTitles = recentTitles,
                modifier = Modifier.weight(1f).padding(end = 8.dp)
            )
        }
        
        // Active Filter Indication (Game Title)
        if (searchFilters.gameTitle != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "絞り込み中: ",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Text(
                    text = searchFilters.gameTitle!!,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.weight(1f),
                    maxLines = 1
                )
                IconButton(onClick = { viewModel.updateFilters(searchFilters.copy(gameTitle = null)) }, modifier = Modifier.size(24.dp)) {
                    Icon(
                        imageVector = androidx.compose.material.icons.Icons.Default.Cancel,
                        contentDescription = "Clear Title Filter",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp)
                    )
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)
        } else if (recentTitles.isNotEmpty() && searchFilters.text.isBlank() && searchFilters.since == null && !searchFilters.isFavorite) {
            // Horizontal Scroller for Title Cards
            androidx.compose.foundation.lazy.LazyRow(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                items(recentTitles) { (title, screenshot) ->
                    Box(
                        modifier = Modifier
                            .width((screenWidthDp / 2.2).dp)
                            .height(100.dp)
                            .clip(androidx.compose.foundation.shape.RoundedCornerShape(12.dp))
                            .clickable { viewModel.updateFilters(searchFilters.copy(gameTitle = title)) }
                    ) {
                        AsyncImage(
                            model = ImageRequest.Builder(LocalContext.current)
                                .data(screenshot.uri)
                                // サムネイル用に小さいサイズを指定してデコードコストを削減
                                .size(with(density) { (screenWidthDp / 3).dp.toPx().roundToInt() },
                                      with(density) { 80.dp.toPx().roundToInt() })
                                .build(),
                            contentDescription = null,
                            contentScale = ContentScale.Crop,
                            modifier = Modifier.fillMaxSize()
                        )
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .background(androidx.compose.ui.graphics.Brush.verticalGradient(
                                    colors = listOf(Color.Transparent, Color.Black.copy(alpha = 0.8f))
                                ))
                        )
                        Text(
                            text = title,
                            color = Color.White,
                            style = MaterialTheme.typography.labelMedium,
                            maxLines = 2,
                            modifier = Modifier
                                .align(Alignment.BottomStart)
                                .padding(8.dp)
                        )
                    }
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)
        }

        if (screenshots.isEmpty()) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(
                        imageVector = androidx.compose.material.icons.Icons.Default.ImageNotSupported,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
                        modifier = Modifier.size(48.dp)
                    )
                    Spacer(modifier = Modifier.height(16.dp))
                    Text("画像が見つかりません。", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(bottom = 24.dp)
            ) {
                sections.forEach { section ->
                    item(key = "header-${section.title}", contentType = "header") {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .background(MaterialTheme.colorScheme.background.copy(alpha = 0.9f))
                                .padding(horizontal = 8.dp, vertical = 12.dp)
                        ) {
                            Text(
                                text = section.title,
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }

                    items(section.rows, key = { row -> row.items.first().screenshot.localId }, contentType = { "grid_row" }) { row ->
                        // リコンポジション時の再計算を防止
                        val rowAspectRatio = remember(row) {
                            row.items.sumOf { it.aspectRatio.toDouble() }.toFloat()
                        }

                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = SPACING, vertical = SPACING / 2),
                            horizontalArrangement = Arrangement.spacedBy(SPACING)
                        ) {
                            row.items.forEach { item ->
                                val isFavorite = item.isFavorite

                                val itemModifier = if (row.isLastRow) {
                                    Modifier.width(item.widthDp.dp)
                                } else {
                                    val itemWeight = item.aspectRatio / rowAspectRatio
                                    Modifier.weight(itemWeight)
                                }
                                
                                Box(modifier = itemModifier) {
                                    ScreenshotGridItem(
                                        screenshot = item.screenshot,
                                        isFavorite = isFavorite,
                                        aspectRatio = item.aspectRatio,
                                        thumbnailHeightPx = thumbnailHeightPx,
                                        onFavoriteToggle = { viewModel.toggleFavorite(item.screenshot.localId, isFavorite) },
                                        onNavigateToDetail = { onNavigateToDetail(item.screenshot.localId) },
                                        sharedTransitionScope = sharedTransitionScope,
                                        animatedVisibilityScope = animatedVisibilityScope
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
fun ScreenshotGridItem(
    screenshot: MediaStoreScreenshot,
    isFavorite: Boolean,
    aspectRatio: Float,
    thumbnailHeightPx: Int,
    onFavoriteToggle: () -> Unit,
    onNavigateToDetail: () -> Unit,
    sharedTransitionScope: SharedTransitionScope,
    animatedVisibilityScope: AnimatedVisibilityScope
) {
    // サムネイル用のピクセルサイズを計算（Coil がダウンサンプリングに使用）
    val thumbnailWidthPx = (thumbnailHeightPx * aspectRatio).roundToInt()

    with(sharedTransitionScope) {
        Box(
            modifier = Modifier
                .aspectRatio(aspectRatio)
                .clip(MaterialTheme.shapes.small)
                .clickable { onNavigateToDetail() }
        ) {
            AsyncImage(
                model = ImageRequest.Builder(LocalContext.current)
                    .data(
                        // サムネイルが事前生成されていればファイルを直接使用（フルデコード回避）
                        if (screenshot.thumbnailPath != null) File(screenshot.thumbnailPath)
                        else screenshot.uri
                    )
                    // サムネイルファイルがない場合のフォールバック: Coil にダウンサンプリングさせる
                    .apply { if (screenshot.thumbnailPath == null) size(thumbnailWidthPx, thumbnailHeightPx) }
                    .build(),
                contentDescription = "Screenshot ${screenshot.windowTitle}",
                contentScale = ContentScale.Crop,
                modifier = Modifier
                    .fillMaxSize()
                    .sharedElement(
                        state = rememberSharedContentState(key = "image-${screenshot.localId}"),
                        animatedVisibilityScope = animatedVisibilityScope
                    )
            )
            
            // お気に入りボタンオーバーレイ
            IconButton(
                onClick = onFavoriteToggle,
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .size(32.dp)
                    .padding(4.dp)
            ) {
                Icon(
                    imageVector = if (isFavorite) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                    contentDescription = "Favorite",
                    tint = if (isFavorite) Color.Red else Color.White
                )
            }
        }
    }
}
