package moe.ryoha.remoterg.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable

// RN版に合わせたzincベースのダークカラースキーム
private val DarkColorScheme = darkColorScheme(
    primary = Purple80,
    secondary = PurpleGrey80,
    tertiary = Pink80,
    background = Zinc950,       // #09090b — RN: bg-zinc-950
    surface = Zinc900,          // #18181b — RN: bg-zinc-900
    surfaceVariant = Zinc800,   // #27272a — RN: bg-zinc-800
    onBackground = Zinc200,     // #e4e4e7 — RN: text-zinc-200
    onSurface = Zinc200,        // #e4e4e7 — RN: text-zinc-200
    onSurfaceVariant = Zinc400, // #a1a1aa — RN: text-zinc-400
    outline = Zinc700,          // #3f3f46 — RN: border-zinc-700
    outlineVariant = Zinc700,   // #3f3f46 — RN: border-zinc-700
)

@Composable
fun RemotergTheme(
    // RN版に合わせて常時ダークテーマを強制
    content: @Composable () -> Unit
) {
    MaterialTheme(
        colorScheme = DarkColorScheme,
        typography = Typography,
        content = content
    )
}