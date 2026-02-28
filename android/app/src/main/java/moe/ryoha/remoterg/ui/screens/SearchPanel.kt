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
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
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
    recentCharacters: List<String>,
    recentSpeakers: List<String>,
    modifier: Modifier = Modifier
) {
    var isExpanded by remember { mutableStateOf(false) }
    var inputText by remember { mutableStateOf("") }
    var showHelp by remember { mutableStateOf(false) }
    
    val focusRequester = remember { FocusRequester() }
    val focusManager = LocalFocusManager.current

    // Sync input with filters object when closed
    LaunchedEffect(filters, isExpanded) {
        if (!isExpanded) {
            inputText = buildQueryString(filters)
        }
    }

    var parentWidth by remember { mutableStateOf(0) }
    val density = LocalDensity.current

    Box(modifier = modifier.onGloballyPositioned { parentWidth = it.size.width }) {
        // Main Search Bar
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(40.dp)
                .clip(RoundedCornerShape(8.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp))
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
            
            BasicTextField(
                value = inputText,
                onValueChange = { 
                    inputText = it
                    onFiltersChanged(parseQueryString(it, filters))
                },
                modifier = Modifier
                    .weight(1f)
                    .focusRequester(focusRequester)
                    .onFocusChanged { focusState ->
                        isExpanded = focusState.isFocused
                        if (!isExpanded) {
                            showHelp = false
                        }
                    },
                textStyle = TextStyle(
                    color = MaterialTheme.colorScheme.onSurface,
                    fontSize = 14.sp
                ),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                singleLine = true,
                decorationBox = { innerTextField ->
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (inputText.isEmpty()) {
                            Text("検索...", color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 14.sp)
                        }
                        innerTextField()
                    }
                }
            )
            
            if (inputText.isNotEmpty()) {
                IconButton(
                    onClick = {
                        inputText = ""
                        onFiltersChanged(SearchFilters())
                        focusManager.clearFocus()
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
                onClick = { 
                    if (showHelp && isExpanded) {
                        showHelp = false
                    } else {
                        showHelp = true
                        focusRequester.requestFocus()
                    }
                },
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
                onDismissRequest = { 
                    focusManager.clearFocus()
                },
                properties = androidx.compose.ui.window.PopupProperties(focusable = false)
            ) {
                Box(
                    modifier = Modifier
                        .width(with(density) { parentWidth.toDp() })
                        .padding(top = 44.dp)
                ) {
                    AnimatedVisibility(
                        visibleState = transitionState,
                        enter = fadeIn() + expandVertically(expandFrom = Alignment.Top),
                        exit = fadeOut() + shrinkVertically(shrinkTowards = Alignment.Top)
                    ) {
                        Card(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable(enabled = false) {}, // absorb clicks inside
                            shape = RoundedCornerShape(12.dp),
                            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                            elevation = CardDefaults.cardElevation(defaultElevation = 8.dp)
                        ) {
                            Column(modifier = Modifier.fillMaxWidth()) {
                                // Suggestions for typed text
                                if (inputText.isNotBlank() && !inputText.contains(Regex("^\\s*(game|chara|text|since|until|is|speaker):"))) {
                                    val rawText = inputText.trim()
                                    Column(modifier = Modifier.padding(12.dp)) {
                                        Text(
                                            text = "検索サジェスト",
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            modifier = Modifier.padding(bottom = 8.dp)
                                        )
                                        
                                        FlowRow(
                                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                                            verticalArrangement = Arrangement.spacedBy(8.dp),
                                            modifier = Modifier.fillMaxWidth()
                                        ) {
                                            SuggestionRow(
                                                icon = Icons.Default.VideogameAsset,
                                                text = "\"$rawText\" をタイトルから探す",
                                                onClick = {
                                                    val newFilters = filters.copy(text = "", gameTitle = rawText)
                                                    onFiltersChanged(newFilters)
                                                    inputText = buildQueryString(newFilters)
                                                    focusManager.clearFocus()
                                                }
                                            )
                                            SuggestionRow(
                                                icon = Icons.Default.RecordVoiceOver,
                                                text = "\"$rawText\" を話し手から探す",
                                                onClick = {
                                                    val newFilters = filters.copy(text = "", speakerText = rawText)
                                                    onFiltersChanged(newFilters)
                                                    inputText = buildQueryString(newFilters)
                                                    focusManager.clearFocus()
                                                }
                                            )
                                            SuggestionRow(
                                                icon = Icons.Default.ChatBubbleOutline,
                                                text = "\"$rawText\" をテキストから探す",
                                                onClick = {
                                                    val newFilters = filters.copy(text = "", dialogText = rawText)
                                                    onFiltersChanged(newFilters)
                                                    inputText = buildQueryString(newFilters)
                                                    focusManager.clearFocus()
                                                }
                                            )
                                            SuggestionRow(
                                                icon = Icons.Default.Person,
                                                text = "\"$rawText\" を登場人物から探す",
                                                onClick = {
                                                    val newFilters = filters.copy(text = "", charaText = rawText)
                                                    onFiltersChanged(newFilters)
                                                    inputText = buildQueryString(newFilters)
                                                    focusManager.clearFocus()
                                                }
                                            )
                                        }
                                    }
                                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                                }

                                // Quick Title Filters
                                if (recentTitles.isNotEmpty()) {
                                    FilterSuggestionGroup(
                                        title = "タイトルで絞り込み",
                                        items = recentTitles,
                                        icon = Icons.Default.VideogameAsset,
                                        itemText = { it.first },
                                        onItemSelected = {
                                            val newFilters = filters.copy(gameTitle = it.first)
                                            onFiltersChanged(newFilters)
                                            inputText = buildQueryString(newFilters)
                                            focusManager.clearFocus()
                                        }
                                    )
                                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                                }

                                // Quick Speaker Filters
                                if (recentSpeakers.isNotEmpty()) {
                                    FilterSuggestionGroup(
                                        title = "話し手で絞り込み",
                                        items = recentSpeakers,
                                        icon = Icons.Default.RecordVoiceOver,
                                        itemText = { it },
                                        onItemSelected = {
                                            val newFilters = filters.copy(speakerText = it)
                                            onFiltersChanged(newFilters)
                                            inputText = buildQueryString(newFilters)
                                            focusManager.clearFocus()
                                        }
                                    )
                                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                                }

                                // Quick Character Filters
                                if (recentCharacters.isNotEmpty()) {
                                    FilterSuggestionGroup(
                                        title = "登場人物で絞り込み",
                                        items = recentCharacters,
                                        icon = Icons.Default.Person,
                                        itemText = { it },
                                        onItemSelected = {
                                            val newFilters = filters.copy(charaText = it)
                                            onFiltersChanged(newFilters)
                                            inputText = buildQueryString(newFilters)
                                            focusManager.clearFocus()
                                        }
                                    )
                                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                                }

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
                                            val newFilters = filters.copy(isFavorite = newVal)
                                            onFiltersChanged(newFilters)
                                            inputText = buildQueryString(newFilters)
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
                                        HelpRow(token = "speaker:\"name\"", desc = "話し手で絞り込み", color = Color(0xFFF97316))
                                        HelpRow(token = "text:\"dialogue\"", desc = "テキストで絞り込み", color = Color(0xFF34D399))
                                        HelpRow(token = "chara:\"name\"", desc = "登場人物で絞り込み", color = Color(0xFFFBBF24))
                                        HelpRow(token = "キーワード", desc = "タイトルやプロセス名で曖昧検索", color = MaterialTheme.colorScheme.onSurface)
                                        Spacer(modifier = Modifier.height(8.dp))
                                        Text(text = "※値にスペースを含まない場合はダブルクォーテーションを省略できます (例: text:こんにちは)", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
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
fun <T> FilterSuggestionGroup(
    title: String,
    items: List<T>,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    itemText: (T) -> String,
    onItemSelected: (T) -> Unit
) {
    var visibleItemCount by remember { mutableIntStateOf(10) }
    
    // Reset visible count when item list changes significantly
    LaunchedEffect(items) {
        visibleItemCount = 10
    }

    Column(modifier = Modifier.padding(12.dp)) {
        Text(
            text = title,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 8.dp)
        )
        
        val visibleItems = items.take(visibleItemCount)
        
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxWidth()
        ) {
            visibleItems.forEach { item ->
                Surface(
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    shape = RoundedCornerShape(8.dp),
                    onClick = { onItemSelected(item) }
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
                            text = itemText(item),
                            color = MaterialTheme.colorScheme.onSurface,
                            fontSize = 14.sp
                        )
                    }
                }
            }
        }
        
        if (items.size > visibleItemCount) {
            TextButton(
                onClick = { visibleItemCount += 20 },
                modifier = Modifier.align(Alignment.CenterHorizontally).padding(top = 8.dp)
            ) {
                Text("さらに読み込む", color = MaterialTheme.colorScheme.primary)
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
        onClick = onClick
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

private val dateFormat = SimpleDateFormat("yyyy-MM-dd", Locale.US)

private fun escapeValue(value: String): String {
    return if (value.contains(" ")) "\"$value\"" else value
}

private fun buildQueryString(filters: SearchFilters): String {
    val parts = mutableListOf<String>()
    if (filters.gameTitle != null) parts.add("game:${escapeValue(filters.gameTitle)}")
    if (filters.charaText != null) parts.add("chara:${escapeValue(filters.charaText)}")
    if (filters.speakerText != null) parts.add("speaker:${escapeValue(filters.speakerText)}")
    if (filters.dialogText != null) parts.add("text:${escapeValue(filters.dialogText)}")
    if (filters.since != null) parts.add("since:${dateFormat.format(Date(filters.since))}")
    if (filters.until != null) parts.add("until:${dateFormat.format(Date(filters.until))}")
    if (filters.isFavorite) parts.add("is:favorite")
    if (filters.text.isNotBlank()) parts.add(filters.text)
    return parts.joinToString(" ")
}

private fun extractFilter(text: String, prefix: String): Pair<String, String?> {
    var newText = text
    // Match quoted value first (e.g. text:"my value")
    val quotedRegex = Regex("$prefix:\"([^\"]+)\"")
    val quotedMatch = quotedRegex.find(newText)
    if (quotedMatch != null) {
        newText = newText.replace(quotedMatch.value, "")
        return Pair(newText, quotedMatch.groupValues[1])
    }
    
    // Match unquoted value next (e.g. text:myvalue) that doesn't contain spaces
    val unquotedRegex = Regex("$prefix:([^\\s]+)")
    val unquotedMatch = unquotedRegex.find(newText)
    if (unquotedMatch != null) {
        newText = newText.replace(unquotedMatch.value, "")
        return Pair(newText, unquotedMatch.groupValues[1])
    }
    
    return Pair(newText, null)
}

private fun parseQueryString(query: String, currentFilters: SearchFilters): SearchFilters {
    var text = query

    // is:favorite
    val isFavorite = if (text.contains("is:favorite")) {
        text = text.replace("is:favorite", "")
        true
    } else {
        false
    }

    // since:YYYY-MM-DD
    var since: Long? = currentFilters.since
    val sinceRegex = Regex("since:([^\\s]+)")
    val sinceMatch = sinceRegex.find(text)
    if (sinceMatch != null) {
        val dateStr = sinceMatch.groupValues[1].replace("/", "-")
        try {
            since = dateFormat.parse(dateStr)?.time
        } catch (e: Exception) {}
        text = text.replace(sinceMatch.value, "")
    }

    // until:YYYY-MM-DD
    var until: Long? = currentFilters.until
    val untilRegex = Regex("until:([^\\s]+)")
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
    }
    
    val (textAfterGame, parsedGameTitle) = extractFilter(text, "game")
    text = textAfterGame
    
    val (textAfterChara, parsedCharaText) = extractFilter(text, "chara")
    text = textAfterChara
    
    val (textAfterSpeaker, parsedSpeakerText) = extractFilter(text, "speaker")
    text = textAfterSpeaker
    
    val (textAfterDialog, parsedDialogText) = extractFilter(text, "text")
    text = textAfterDialog

    return SearchFilters(
        text = text.trim().replace(Regex("\\s+"), " "), // Normalize spaces
        since = since,
        until = until,
        gameTitle = parsedGameTitle,
        charaText = parsedCharaText,
        speakerText = parsedSpeakerText,
        dialogText = parsedDialogText,
        isFavorite = isFavorite
    )
}
