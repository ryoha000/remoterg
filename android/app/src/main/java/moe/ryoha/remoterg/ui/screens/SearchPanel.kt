@file:OptIn(ExperimentalMaterial3Api::class)

package moe.ryoha.remoterg.ui.screens

import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.HelpOutline
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import moe.ryoha.remoterg.data.repository.MediaStoreScreenshot
import moe.ryoha.remoterg.ui.viewmodel.SearchFilters
import java.text.SimpleDateFormat
import java.util.*

@Composable
fun SearchPanel(
    filters: SearchFilters,
    onFiltersChanged: (SearchFilters) -> Unit,
    recentTitles: List<Pair<String, MediaStoreScreenshot>>,
    modifier: Modifier = Modifier
) {
    var isExpanded by remember { mutableStateOf(false) }
    var inputText by remember { mutableStateOf("") }
    var showHelp by remember { mutableStateOf(false) }
    
    val focusRequester = remember { FocusRequester() }

    // Sync input with filters object when closed
    LaunchedEffect(filters, isExpanded) {
        if (!isExpanded) {
            inputText = buildQueryString(filters)
        }
    }

    // Determine active tokens
    val tokens = parseTokens(filters)
    val hasActiveFilters = tokens.isNotEmpty() || filters.text.isNotBlank()

    Box(modifier = modifier) {
        // Collapsed Search Bar / Top interaction area
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(40.dp)
                .clip(RoundedCornerShape(8.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp))
                .clickable { 
                    isExpanded = true
                }
                .padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(
                imageVector = Icons.Default.Search,
                contentDescription = "Search",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(18.dp)
            )
            
            Spacer(modifier = Modifier.width(8.dp))
            
            if (tokens.isNotEmpty()) {
                LazyRow(
                    modifier = Modifier.weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    items(tokens) { token ->
                        TokenChip(
                            token = token,
                            onRemove = {
                                val newFilters = removeTokenFromFilters(filters, token)
                                onFiltersChanged(newFilters)
                                inputText = buildQueryString(newFilters)
                            }
                        )
                    }
                    if (filters.text.isNotBlank()) {
                        item {
                            Text(
                                text = filters.text,
                                color = MaterialTheme.colorScheme.onSurface,
                                fontSize = 14.sp,
                                maxLines = 1,
                                modifier = Modifier.padding(start = 4.dp)
                            )
                        }
                    }
                }
            } else {
                Text(
                    text = if (filters.text.isNotBlank()) filters.text else "検索...",
                    color = if (filters.text.isNotBlank()) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 14.sp,
                    maxLines = 1,
                    modifier = Modifier.weight(1f)
                )
            }
            
            if (hasActiveFilters) {
                IconButton(
                    onClick = {
                        onFiltersChanged(SearchFilters())
                    },
                    modifier = Modifier.size(24.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.Cancel,
                        contentDescription = "Clear",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp)
                    )
                }
            }
            
            IconButton(
                onClick = { showHelp = !showHelp },
                modifier = Modifier.size(24.dp)
            ) {
                Icon(
                    imageVector = Icons.Outlined.HelpOutline,
                    contentDescription = "Help",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(16.dp)
                )
            }
        }
        
        // Expanded Panel Overlay
        AnimatedVisibility(
            visible = isExpanded,
            enter = fadeIn() + expandVertically(expandFrom = Alignment.Top),
            exit = fadeOut() + shrinkVertically(shrinkTowards = Alignment.Top)
        ) {
            // Need a full screen backdrop to catch clicks outside
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(Unit) {
                        detectTapGestures(onTap = { isExpanded = false })
                    }
            ) {
                Card(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 44.dp)
                        .clickable(enabled = false) {}, // absorb clicks inside
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    elevation = CardDefaults.cardElevation(defaultElevation = 8.dp)
                ) {
                    Column(modifier = Modifier.fillMaxWidth()) {
                        // Editable text field row
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(12.dp)
                                .clip(RoundedCornerShape(8.dp))
                                .background(MaterialTheme.colorScheme.surfaceVariant)
                                .padding(horizontal = 12.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(
                                imageVector = Icons.Default.Search,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.size(18.dp)
                            )
                            Spacer(modifier = Modifier.width(8.dp))
                            BasicTextField(
                                value = inputText,
                                onValueChange = { 
                                    inputText = it
                                    onFiltersChanged(parseQueryString(it, filters))
                                },
                                modifier = Modifier
                                    .weight(1f)
                                    .focusRequester(focusRequester),
                                textStyle = TextStyle(
                                    color = MaterialTheme.colorScheme.onSurface,
                                    fontSize = 14.sp
                                ),
                                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                                singleLine = true
                            )
                            IconButton(
                                onClick = { isExpanded = false },
                                modifier = Modifier.size(24.dp)
                            ) {
                                Icon(
                                    imageVector = Icons.Default.KeyboardArrowUp,
                                    contentDescription = "Close",
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                        }
                        
                        LaunchedEffect(isExpanded) {
                            if (isExpanded) {
                                focusRequester.requestFocus()
                            }
                        }

                        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

                        // Quick Date Filters
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                text = "日付で絞り込み",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(bottom = 8.dp)
                            )
                            
                            val cal = Calendar.getInstance()
                            cal.set(Calendar.HOUR_OF_DAY, 0)
                            cal.set(Calendar.MINUTE, 0)
                            cal.set(Calendar.SECOND, 0)
                            cal.set(Calendar.MILLISECOND, 0)
                            val todayStart = cal.timeInMillis
                            
                            cal.add(Calendar.DAY_OF_YEAR, -1)
                            val yesterdayStart = cal.timeInMillis
                            
                            val endOfDay = todayStart + 86400000L - 1L
                            val yesterdayEnd = todayStart - 1L
                            
                            cal.timeInMillis = todayStart
                            cal.add(Calendar.DAY_OF_YEAR, -7)
                            val past7Days = cal.timeInMillis
                            
                            cal.timeInMillis = todayStart
                            cal.add(Calendar.DAY_OF_YEAR, -30)
                            val past30Days = cal.timeInMillis

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                DateSuggestionButton(
                                    label = "今日",
                                    icon = Icons.Default.Today,
                                    onClick = { 
                                        onFiltersChanged(filters.copy(since = todayStart, until = endOfDay))
                                        isExpanded = false
                                    }
                                )
                                DateSuggestionButton(
                                    label = "昨日",
                                    icon = Icons.Default.CalendarToday,
                                    onClick = { 
                                        onFiltersChanged(filters.copy(since = yesterdayStart, until = yesterdayEnd))
                                        isExpanded = false
                                    }
                                )
                            }
                            Spacer(modifier = Modifier.height(8.dp))
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                DateSuggestionButton(
                                    label = "過去7日間",
                                    icon = Icons.Default.Schedule,
                                    onClick = { 
                                        onFiltersChanged(filters.copy(since = past7Days, until = endOfDay))
                                        isExpanded = false
                                    }
                                )
                                DateSuggestionButton(
                                    label = "過去30日間",
                                    icon = Icons.Default.DateRange,
                                    onClick = { 
                                        onFiltersChanged(filters.copy(since = past30Days, until = endOfDay))
                                        isExpanded = false
                                    }
                                )
                            }
                        }

                        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

                        // Quick Filters
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                text = "クイックフィルター",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(bottom = 8.dp)
                            )
                            
                            Surface(
                                color = if (filters.isFavorite) Color(0x33F91980) else MaterialTheme.colorScheme.surfaceVariant,
                                border = if (filters.isFavorite) BorderStroke(1.dp, Color(0x4DF91980)) else null,
                                shape = RoundedCornerShape(8.dp),
                                onClick = {
                                    val newVal = !filters.isFavorite
                                    val newText = if (newVal) {
                                        if (!inputText.contains("is:favorite")) "$inputText is:favorite".trim() else inputText
                                    } else {
                                        inputText.replace("is:favorite", "").trim()
                                    }
                                    inputText = newText
                                    onFiltersChanged(filters.copy(isFavorite = newVal))
                                }
                            ) {
                                Row(
                                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Icon(
                                        imageVector = if (filters.isFavorite) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                                        contentDescription = null,
                                        tint = if (filters.isFavorite) Color(0xFFF91980) else MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.size(16.dp)
                                    )
                                    Spacer(modifier = Modifier.width(8.dp))
                                    Text(
                                        text = "お気に入りのみ",
                                        color = if (filters.isFavorite) Color(0xFFF472B6) else MaterialTheme.colorScheme.onSurface,
                                        fontSize = 14.sp
                                    )
                                }
                            }
                        }

                        // Help section
                        AnimatedVisibility(visible = showHelp) {
                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(MaterialTheme.colorScheme.surfaceColorAtElevation(1.dp))
                                    .padding(12.dp)
                            ) {
                                Text(
                                    text = "検索の使い方",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(bottom = 8.dp)
                                )
                                HelpRow(token = "since:2024-01-01", desc = "指定日以降の画像", color = Color(0xFF60A5FA))
                                HelpRow(token = "until:2024-01-31", desc = "指定日以前の画像", color = Color(0xFF60A5FA))
                                HelpRow(token = "is:favorite", desc = "お気に入りのみ表示", color = Color(0xFFF472B6))
                                HelpRow(token = "キーワード", desc = "ゲームタイトルなどで検索", color = MaterialTheme.colorScheme.onSurface)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun DateSuggestionButton(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(8.dp),
        onClick = onClick,
        modifier = Modifier.wrapContentWidth()
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(16.dp)
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = label,
                color = MaterialTheme.colorScheme.onSurface,
                fontSize = 14.sp
            )
        }
    }
}

@Composable
fun HelpRow(token: String, desc: String, color: Color) {
    Row(modifier = Modifier.padding(vertical = 2.dp)) {
        Text(text = token, color = color, fontSize = 12.sp, fontWeight = FontWeight.Medium)
        Text(text = " - $desc", color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 12.sp)
    }
}

enum class TokenType { Text, Since, Until, Favorite, Game }

data class SearchToken(
    val type: TokenType,
    val value: String,
    val displayValue: String
)

@Composable
fun TokenChip(token: SearchToken, onRemove: () -> Unit) {
    val (bgColor, borderColor, textColor) = when (token.type) {
        TokenType.Since, TokenType.Until -> Triple(Color(0x333B82F6), Color(0x4D3B82F6), Color(0xFF60A5FA))
        TokenType.Favorite -> Triple(Color(0x33EC4899), Color(0x4DEC4899), Color(0xFFF472B6))
        TokenType.Game -> Triple(Color(0x33A855F7), Color(0x4DA855F7), Color(0xFFC084FC))
        TokenType.Text -> Triple(MaterialTheme.colorScheme.surfaceVariant, MaterialTheme.colorScheme.outline, MaterialTheme.colorScheme.onSurface)
    }

    Row(
        modifier = Modifier
            .background(bgColor, RoundedCornerShape(4.dp))
            .border(1.dp, borderColor, RoundedCornerShape(4.dp))
            .padding(horizontal = 6.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(text = token.displayValue, color = textColor, fontSize = 12.sp)
        Spacer(modifier = Modifier.width(4.dp))
        Icon(
            imageVector = Icons.Default.Cancel,
            contentDescription = "Remove",
            tint = textColor.copy(alpha = 0.7f),
            modifier = Modifier
                .size(14.dp)
                .clickable { onRemove() }
        )
    }
}

private val dateFormat = SimpleDateFormat("yyyy-MM-dd", Locale.US)

private fun parseTokens(filters: SearchFilters): List<SearchToken> {
    val tokens = mutableListOf<SearchToken>()
    if (filters.since != null) {
        val dateStr = dateFormat.format(Date(filters.since))
        tokens.add(SearchToken(TokenType.Since, "since:$dateStr", "以降: $dateStr"))
    }
    if (filters.until != null) {
        val dateStr = dateFormat.format(Date(filters.until))
        tokens.add(SearchToken(TokenType.Until, "until:$dateStr", "以前: $dateStr"))
    }
    if (filters.isFavorite) {
        tokens.add(SearchToken(TokenType.Favorite, "is:favorite", "お気に入り"))
    }
    if (filters.gameTitle != null) {
        tokens.add(SearchToken(TokenType.Game, "game:\"${filters.gameTitle}\"", filters.gameTitle))
    }
    return tokens
}

private fun buildQueryString(filters: SearchFilters): String {
    val parts = mutableListOf<String>()
    if (filters.text.isNotBlank()) parts.add(filters.text)
    if (filters.since != null) parts.add("since:${dateFormat.format(Date(filters.since))}")
    if (filters.until != null) parts.add("until:${dateFormat.format(Date(filters.until))}")
    if (filters.isFavorite) parts.add("is:favorite")
    if (filters.gameTitle != null) parts.add("game:\"${filters.gameTitle}\"")
    return parts.joinToString(" ")
}

private fun parseQueryString(query: String, currentFilters: SearchFilters): SearchFilters {
    var text = query
    var since: Long? = currentFilters.since
    var until: Long? = currentFilters.until
    var isFavorite = currentFilters.isFavorite
    val gameTitle = currentFilters.gameTitle

    // is:favorite
    if (text.contains("is:favorite")) {
        isFavorite = true
        text = text.replace("is:favorite", "")
    } else {
        isFavorite = false
    }

    // since:YYYY-MM-DD
    val sinceRegex = Regex("since:(\\d{4}[-/]\\d{2}[-/]\\d{2})")
    val sinceMatch = sinceRegex.find(text)
    if (sinceMatch != null) {
        val dateStr = sinceMatch.groupValues[1].replace("/", "-")
        try {
            since = dateFormat.parse(dateStr)?.time
        } catch (e: Exception) {}
        text = text.replace(sinceMatch.value, "")
    } else {
        since = null
    }

    // until:YYYY-MM-DD
    val untilRegex = Regex("until:(\\d{4}[-/]\\d{2}[-/]\\d{2})")
    val untilMatch = untilRegex.find(text)
    if (untilMatch != null) {
        val dateStr = untilMatch.groupValues[1].replace("/", "-")
        try {
            val d = dateFormat.parse(dateStr)
            if (d != null) {
                // End of day
                until = d.time + 86400000L - 1L
            }
        } catch (e: Exception) {}
        text = text.replace(untilMatch.value, "")
    } else {
        until = null
    }
    
    // game:"title"
    val gameRegex = Regex("game:\"([^\"]+)\"")
    val gameMatch = gameRegex.find(text)
    val parsedGameTitle = if (gameMatch != null) {
        text = text.replace(gameMatch.value, "")
        gameMatch.groupValues[1]
    } else {
        gameTitle
    }

    return SearchFilters(
        text = text.trim(),
        since = since,
        until = until,
        gameTitle = parsedGameTitle,
        isFavorite = isFavorite
    )
}

private fun removeTokenFromFilters(filters: SearchFilters, token: SearchToken): SearchFilters {
    return when (token.type) {
        TokenType.Since -> filters.copy(since = null)
        TokenType.Until -> filters.copy(until = null)
        TokenType.Favorite -> filters.copy(isFavorite = false)
        TokenType.Game -> filters.copy(gameTitle = null)
        TokenType.Text -> filters.copy(text = "")
    }
}
