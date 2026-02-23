package moe.ryoha.remoterg.ui.screens

import android.Manifest
import android.os.Build
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowForward
import androidx.compose.material.icons.filled.Monitor
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import moe.ryoha.remoterg.ui.viewmodel.ConnectViewModel
import kotlinx.coroutines.delay

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConnectScreen(
    viewModel: ConnectViewModel,
    onConnect: (url: String, codec: String) -> Unit,
    onNavigateToGallery: () -> Unit
) {
    val context = LocalContext.current
    var sessionId by remember { mutableStateOf("fixed") }
    var selectedCodec by remember { mutableStateOf("av1") }
    
    var serverMode by remember { mutableStateOf("local") }
    var customServerUrl by remember { mutableStateOf("ws://192.168.0.10:8787") }

    var showSettingsDialog by remember { mutableStateOf(false) }

    // ランタイムパーミッションリクエスト用ランチャー
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { isGranted ->
        if (isGranted) {
            viewModel.clearAllData { success ->
                if (success) {
                    Toast.makeText(context, "全てのスクリーンショットを削除しました", Toast.LENGTH_SHORT).show()
                } else {
                    Toast.makeText(context, "スクリーンショットの削除に失敗しました", Toast.LENGTH_SHORT).show()
                }
            }
        } else {
            Toast.makeText(context, "権限が必要です", Toast.LENGTH_SHORT).show()
        }
    }

    fun performClearAllData() {
        val permission = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            Manifest.permission.READ_MEDIA_IMAGES
        } else {
            Manifest.permission.READ_EXTERNAL_STORAGE
        }
        permissionLauncher.launch(permission)
    }

    // Ping Animation for status indicator
    val alphaAnim = rememberInfiniteTransition(label = "ping")
    val pingAlpha by alphaAnim.animateFloat(
        initialValue = 0.8f,
        targetValue = 0.0f,
        animationSpec = infiniteRepeatable(
            animation = tween(1000, easing = LinearEasing),
            repeatMode = RepeatMode.Restart
        ),
        label = "ping_alpha"
    )

    val pingScale by alphaAnim.animateFloat(
        initialValue = 1f,
        targetValue = 2.5f,
        animationSpec = infiniteRepeatable(
            animation = tween(1000, easing = LinearOutSlowInEasing),
            repeatMode = RepeatMode.Restart
        ),
        label = "ping_scale"
    )

    Box(modifier = Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(24.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            // Monitor Icon Background
            Box(
                modifier = Modifier
                    .size(56.dp)
                    .clip(RoundedCornerShape(16.dp))
                    .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.1f)),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    imageVector = Icons.Default.Monitor,
                    contentDescription = null,
                    modifier = Modifier.size(32.dp),
                    tint = MaterialTheme.colorScheme.primary
                )
            }
            
            Spacer(modifier = Modifier.height(16.dp))
            
            // Title
            Text(
                text = "RemoteRG",
                style = MaterialTheme.typography.headlineLarge.copy(
                    fontWeight = FontWeight.Bold,
                    letterSpacing = (-0.5).sp
                ),
                textAlign = TextAlign.Center
            )
            
            Spacer(modifier = Modifier.height(8.dp))
            
            // Subtitle
            Text(
                text = "High-performance remote gaming",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center
            )

            Spacer(modifier = Modifier.height(24.dp))

            // Status Indicator
            Surface(
                shape = CircleShape,
                color = MaterialTheme.colorScheme.secondaryContainer,
                modifier = Modifier.padding(bottom = 32.dp)
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.Center
                ) {
                    Box(contentAlignment = Alignment.Center, modifier = Modifier.size(8.dp)) {
                        // Ping Effect
                        Box(
                            modifier = Modifier
                                .size(8.dp)
                                .scale(pingScale)
                                .clip(CircleShape)
                                .background(Color(0xFF22C55E).copy(alpha = pingAlpha)) // green-500
                        )
                        // Inner Dot
                        Box(
                            modifier = Modifier
                                .size(8.dp)
                                .clip(CircleShape)
                                .background(Color(0xFF22C55E)) // green-500
                        )
                    }
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        text = "System Operational",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSecondaryContainer
                    )
                }
            }

            // Buttons Area
            Column(
                modifier = Modifier.widthIn(max = 320.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                // Connect Button
                Button(
                    onClick = {
                        val baseUrl = when (serverMode) {
                            "local" -> "ws://10.0.2.2:8787"
                            "remote" -> "wss://remoterg.the7uya.workers.dev"
                            else -> customServerUrl.trimEnd('/')
                        }
                        val url = "$baseUrl/api/signal?session_id=$sessionId&role=viewer"
                        onConnect(url, selectedCodec)
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(48.dp),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Text(
                        text = "Connect",
                        style = MaterialTheme.typography.titleMedium
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Icon(
                        imageVector = Icons.Default.ArrowForward,
                        contentDescription = null,
                        modifier = Modifier.size(18.dp)
                    )
                }

                Spacer(modifier = Modifier.height(12.dp))

                // View Gallery Button
                OutlinedButton(
                    onClick = onNavigateToGallery,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(48.dp),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Text(
                        text = "View Gallery",
                        style = MaterialTheme.typography.titleMedium
                    )
                }
                
                Spacer(modifier = Modifier.height(16.dp))

                // Settings Button (Popover-like behavior via Dialog)
                TextButton(
                    onClick = { showSettingsDialog = true },
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                ) {
                    Icon(
                        imageVector = Icons.Default.Settings,
                        contentDescription = "Settings",
                        modifier = Modifier.size(18.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text("Connection Settings")
                }
            }
        }

        // Settings Dialog
        if (showSettingsDialog) {
            AlertDialog(
                onDismissRequest = { showSettingsDialog = false },
                title = { Text("Settings") },
                text = {
                    Column {
                        // Signaling Server 選択
                        Text(
                            text = "Signaling Server",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(bottom = 6.dp)
                        )
                        
                        var serverDropdownExpanded by remember { mutableStateOf(false) }
                        val serverOptions = listOf("local" to "Local (10.0.2.2)", "remote" to "Remote", "custom" to "Custom")
                        
                        ExposedDropdownMenuBox(
                            expanded = serverDropdownExpanded,
                            onExpandedChange = { serverDropdownExpanded = it },
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(bottom = if (serverMode == "custom") 8.dp else 16.dp)
                        ) {
                            OutlinedTextField(
                                value = serverOptions.find { it.first == serverMode }?.second ?: "",
                                onValueChange = {},
                                readOnly = true,
                                singleLine = true,
                                trailingIcon = {
                                    ExposedDropdownMenuDefaults.TrailingIcon(expanded = serverDropdownExpanded)
                                },
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .menuAnchor(MenuAnchorType.PrimaryNotEditable),
                                shape = RoundedCornerShape(8.dp)
                            )
                            ExposedDropdownMenu(
                                expanded = serverDropdownExpanded,
                                onDismissRequest = { serverDropdownExpanded = false }
                            ) {
                                serverOptions.forEach { (key, label) ->
                                    DropdownMenuItem(
                                        text = {
                                            Text(
                                                text = label,
                                                style = MaterialTheme.typography.bodyMedium
                                            )
                                        },
                                        onClick = {
                                            serverMode = key
                                            serverDropdownExpanded = false
                                        }
                                    )
                                }
                            }
                        }

                        if (serverMode == "custom") {
                            OutlinedTextField(
                                value = customServerUrl,
                                onValueChange = { customServerUrl = it },
                                placeholder = { Text("ws://192.168.0.10:8787") },
                                singleLine = true,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(bottom = 16.dp),
                                shape = RoundedCornerShape(8.dp)
                            )
                        }

                        // Session ID 入力欄
                        Text(
                            text = "Session ID",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(bottom = 6.dp)
                        )
                        OutlinedTextField(
                            value = sessionId,
                            onValueChange = { sessionId = it },
                            placeholder = { Text("Enter Session ID") },
                            singleLine = true,
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(bottom = 16.dp),
                            shape = RoundedCornerShape(8.dp)
                        )

                        // Codec 選択
                        Text(
                            text = "Codec",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(bottom = 6.dp)
                        )
                        
                        var codecDropdownExpanded by remember { mutableStateOf(false) }
                        val codecOptions = listOf("h264", "vp8", "vp9", "av1")
                        
                        ExposedDropdownMenuBox(
                            expanded = codecDropdownExpanded,
                            onExpandedChange = { codecDropdownExpanded = it },
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(bottom = 24.dp)
                        ) {
                            OutlinedTextField(
                                value = selectedCodec.uppercase(),
                                onValueChange = {},
                                readOnly = true,
                                singleLine = true,
                                trailingIcon = {
                                    ExposedDropdownMenuDefaults.TrailingIcon(expanded = codecDropdownExpanded)
                                },
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .menuAnchor(MenuAnchorType.PrimaryNotEditable),
                                shape = RoundedCornerShape(8.dp)
                            )
                            ExposedDropdownMenu(
                                expanded = codecDropdownExpanded,
                                onDismissRequest = { codecDropdownExpanded = false }
                            ) {
                                codecOptions.forEach { codec ->
                                    DropdownMenuItem(
                                        text = {
                                            Text(
                                                text = codec.uppercase(),
                                                style = MaterialTheme.typography.bodyMedium
                                            )
                                        },
                                        onClick = {
                                            selectedCodec = codec
                                            codecDropdownExpanded = false
                                        }
                                    )
                                }
                            }
                        }

                        // Debug Buttons section
                        Divider(modifier = Modifier.padding(vertical = 12.dp))
                        Text(
                            text = "Debug Actions",
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.tertiary,
                            modifier = Modifier.padding(bottom = 12.dp)
                        )

                        // [Debug] サムネイル一括生成ボタン
                        Button(
                            onClick = {
                                Toast.makeText(context, "サムネイルの生成を開始します...", Toast.LENGTH_SHORT).show()
                                viewModel.generateAllThumbnails { count ->
                                    Toast.makeText(context, "${count}枚のサムネイルを生成しました", Toast.LENGTH_SHORT).show()
                                }
                            },
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(bottom = 8.dp),
                            shape = RoundedCornerShape(8.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.tertiary
                            )
                        ) {
                            Text(
                                text = "サムネイル一括生成",
                                modifier = Modifier.padding(vertical = 4.dp)
                            )
                        }

                        // Delete All Data ボタン
                        Button(
                            onClick = { performClearAllData() },
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(8.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.error
                            )
                        ) {
                            Text(
                                text = "全データ削除",
                                modifier = Modifier.padding(vertical = 4.dp)
                            )
                        }
                    }
                },
                confirmButton = {
                    TextButton(onClick = { showSettingsDialog = false }) {
                        Text("Close")
                    }
                }
            )
        }
    }
}


