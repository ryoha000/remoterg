@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package moe.ryoha.remoterg.ui.screens

import androidx.compose.animation.*
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
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

    var parentWidth by remember { mutableStateOf(0) }
    val density = LocalDensity.current

    Box(modifier = modifier.onGloballyPositioned { parentWidth = it.size.width }) {
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
        val transitionState = remember { androidx.compose.animation.core.MutableTransitionState(false) }
        transitionState.targetState = isExpanded

        if (transitionState.currentState || transitionState.targetState) {
            androidx.compose.ui.window.Popup(
                alignment = Alignment.TopStart,
                onDismissRequest = { isExpanded = false },
                properties = androidx.compose.ui.window.PopupProperties(focusable = true)
            ) {
                Box(
                    modifier = Modifier.width(with(density) { parentWidth.toDp() })
                ) {
                    AnimatedVisibility(
                        visibleState = transitionState,
                        enter = fadeIn() + expandVertically(expandFrom = Alignment.Top),
                        exit = fadeOut() + shrinkVertically(shrinkTowards = Alignment.Top)
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

                        // Suggestions for typed text
                        if (inputText.isNotBlank() && !inputText.contains(Regex("^\\s*(game|chara|text|since|until|is):"))) {
                            val rawText = inputText.trim()
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text(
                                    text = "検索サジェスト",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(bottom = 8.dp)
                                )
                                
                                SuggestionRow(
                                    icon = Icons.Default.VideogameAsset,
                                    text = "\"$rawText\" をタイトルから探す",
                                    onClick = {
                                        onFiltersChanged(filters.copy(text = "", gameTitle = rawText))
                                        isExpanded = false
                                        inputText = ""
                                    }
                                )
                                Spacer(modifier = Modifier.height(4.dp))
                                SuggestionRow(
                                    icon = Icons.Default.Person,
                                    text = "\"$rawText\" をキャラ名(AI)から探す",
                                    onClick = {
                                        onFiltersChanged(filters.copy(text = "", charaText = rawText))
                                        isExpanded = false
                                        inputText = ""
                                    }
                                )
                                Spacer(modifier = Modifier.height(4.dp))
                                SuggestionRow(
                                    icon = Icons.Default.ChatBubbleOutline,
                                    text = "\"$rawText\" をテキスト(AI)から探す",
                                    onClick = {
                                        onFiltersChanged(filters.copy(text = "", dialogText = rawText))
                                        isExpanded = false
                                        inputText = ""
                                    }
                                )
                            }
                            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                        }

                        // Quick Title Filters
                        var visibleTitleCount by remember { mutableIntStateOf(10) }
                        
                        LaunchedEffect(isExpanded) {
                            if (!isExpanded) {
                                visibleTitleCount = 10
                            }
                        }

                        if (recentTitles.isNotEmpty()) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text(
                                    text = "タイトルで絞り込み",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(bottom = 8.dp)
                                )
                                
                                val visibleTitles = recentTitles.take(visibleTitleCount)
                                
                                FlowRow(
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                    verticalArrangement = Arrangement.spacedBy(8.dp),
                                    modifier = Modifier.fillMaxWidth()
                                ) {
                                    visibleTitles.forEach { (title, _) ->
                                        Surface(
                                            color = MaterialTheme.colorScheme.surfaceVariant,
                                            shape = RoundedCornerShape(8.dp),
                                            onClick = { 
                                                onFiltersChanged(filters.copy(gameTitle = title))
                                                isExpanded = false
                                            }
                                        ) {
                                            Row(
                                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                                                verticalAlignment = Alignment.CenterVertically
                                            ) {
                                                Icon(
                                                    imageVector = Icons.Default.VideogameAsset,
                                                    contentDescription = null,
                                                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                                    modifier = Modifier.size(16.dp)
                                                )
                                                Spacer(modifier = Modifier.width(8.dp))
                                                Text(
                                                    text = title,
                                                    color = MaterialTheme.colorScheme.onSurface,
                                                    fontSize = 14.sp
                                                )
                                            }
                                        }
                                    }
                                }
                                
                                if (recentTitles.size > visibleTitleCount) {
                                    TextButton(
                                        onClick = { visibleTitleCount += 20 },
                                        modifier = Modifier.align(Alignment.CenterHorizontally).padding(top = 8.dp)
                                    ) {
                                        Text("さらに読み込む", color = MaterialTheme.colorScheme.primary)
                                    }
                                }
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
                                HelpRow(token = "is:favorite", desc = "お気に入りのみ表示", color = Color(0xFFF472B6))
                                HelpRow(token = "game:\"title\"", desc = "ゲームタイトルで絞り込み", color = Color(0xFFC084FC))
                                HelpRow(token = "chara:\"name\"", desc = "AI分析のキャラ名で絞り込み", color = Color(0xFFFBBF24))
                                HelpRow(token = "text:\"dialogue\"", desc = "AI分析のテキストで絞り込み", color = Color(0xFF34D399))
                                HelpRow(token = "キーワード", desc = "タイトルやプロセス名で曖昧検索", color = MaterialTheme.colorScheme.onSurface)
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
@Composable
fun HelpRow(token: String, desc: String, color: Color) {
    Row(modifier = Modifier.padding(vertical = 2.dp)) {
        Text(text = token, color = color, fontSize = 12.sp, fontWeight = FontWeight.Medium)
        Text(text = " - $desc", color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 12.sp)
    }
}

@Composable
fun SuggestionRow(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String, onClick: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(8.dp),
        onClick = onClick,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(18.dp)
            )
            Spacer(modifier = Modifier.width(12.dp))
            Text(
                text = text,
                color = MaterialTheme.colorScheme.onSurface,
                fontSize = 14.sp
            )
        }
    }
}

enum class TokenType { Text, Since, Until, Favorite, Game, Chara, TextFilter }

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
        TokenType.Chara -> Triple(Color(0x33F59E0B), Color(0x4DF59E0B), Color(0xFFFBBF24))
        TokenType.TextFilter -> Triple(Color(0x3310B981), Color(0x4D10B981), Color(0xFF34D399))
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
    if (filters.charaText != null) {
        tokens.add(SearchToken(TokenType.Chara, "chara:\"${filters.charaText}\"", "👤 ${filters.charaText}"))
    }
    if (filters.dialogText != null) {
        tokens.add(SearchToken(TokenType.TextFilter, "text:\"${filters.dialogText}\"", "💬 ${filters.dialogText}"))
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
    if (filters.charaText != null) parts.add("chara:\"${filters.charaText}\"")
    if (filters.dialogText != null) parts.add("text:\"${filters.dialogText}\"")
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
        currentFilters.gameTitle
    }

    // chara:"name"
    val charaRegex = Regex("chara:\"([^\"]+)\"")
    val charaMatch = charaRegex.find(text)
    val parsedCharaText = if (charaMatch != null) {
        text = text.replace(charaMatch.value, "")
        charaMatch.groupValues[1]
    } else {
        currentFilters.charaText
    }

    // text:"dialogue"
    val textRegex = Regex("text:\"([^\"]+)\"")
    val textMatch = textRegex.find(text)
    val parsedDialogText = if (textMatch != null) {
        text = text.replace(textMatch.value, "")
        textMatch.groupValues[1]
    } else {
        currentFilters.dialogText
    }

    return SearchFilters(
        text = text.trim(),
        since = since,
        until = until,
        gameTitle = parsedGameTitle,
        charaText = parsedCharaText,
        dialogText = parsedDialogText,
        isFavorite = isFavorite
    )
}

private fun removeTokenFromFilters(filters: SearchFilters, token: SearchToken): SearchFilters {
    return when (token.type) {
        TokenType.Since -> filters.copy(since = null)
        TokenType.Until -> filters.copy(until = null)
        TokenType.Favorite -> filters.copy(isFavorite = false)
        TokenType.Game -> filters.copy(gameTitle = null)
        TokenType.Chara -> filters.copy(charaText = null)
        TokenType.TextFilter -> filters.copy(dialogText = null)
        TokenType.Text -> filters.copy(text = "")
    }
}
