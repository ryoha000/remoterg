package moe.ryoha.remoterg

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import dagger.hilt.android.AndroidEntryPoint
import moe.ryoha.remoterg.ui.theme.RemotergTheme
import moe.ryoha.remoterg.ui.screens.ConnectScreen
import moe.ryoha.remoterg.ui.screens.GalleryScreen
import moe.ryoha.remoterg.ui.screens.ViewerScreen
import android.net.Uri
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SharedTransitionLayout
import moe.ryoha.remoterg.ui.screens.GalleryDetailScreen
import moe.ryoha.remoterg.ui.viewmodel.ViewerViewModel
import androidx.compose.animation.core.tween
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally

@AndroidEntryPoint
class MainActivity : ComponentActivity() {
    @OptIn(ExperimentalSharedTransitionApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            RemotergTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    val navController = rememberNavController()

                    // SharedTransitionLayout を Gallery ↔ GalleryDetail 間のみに限定
                    // Viewer 画面（WebRTC映像を毎フレームレンダリング）を含めると
                    // SharedTransition のオーバーヘッドがフレームドロップの原因になる
                    SharedTransitionLayout {
                        NavHost(navController = navController, startDestination = "connect") {
                            composable("connect") {
                                val connectViewModel: moe.ryoha.remoterg.ui.viewmodel.ConnectViewModel = hiltViewModel()
                                ConnectScreen(
                                    viewModel = connectViewModel,
                                    onConnect = { url, codec ->
                                        val encodedUrl = Uri.encode(url)
                                        val encodedCodec = Uri.encode(codec)
                                        navController.navigate("viewer?url=$encodedUrl&codec=$encodedCodec")
                                    },
                                    onNavigateToGallery = {
                                        navController.navigate("gallery")
                                    }
                                )
                            }
                            composable("viewer?url={url}&codec={codec}") { backStackEntry ->
                                val url = Uri.decode(backStackEntry.arguments?.getString("url") ?: "")
                                val codec = Uri.decode(backStackEntry.arguments?.getString("codec") ?: "h264")
                                val viewerViewModel: ViewerViewModel = hiltViewModel()

                                // Viewer 画面は SharedTransition を使用しないためスコープを渡さない
                                ViewerScreen(
                                    signalingUrl = url,
                                    codec = codec,
                                    viewModel = viewerViewModel,
                                    onNavigateBack = {
                                        navController.popBackStack()
                                    },
                                    onNavigateToGallery = {
                                        navController.navigate("gallery")
                                    }
                                )
                            }
                            composable(
                                "gallery",
                                enterTransition = {
                                    slideInHorizontally(
                                        initialOffsetX = { fullWidth -> fullWidth },
                                        animationSpec = tween(300)
                                    )
                                },
                                popExitTransition = {
                                    slideOutHorizontally(
                                        targetOffsetX = { fullWidth -> fullWidth },
                                        animationSpec = tween(300)
                                    )
                                }
                            ) {
                                GalleryScreen(
                                    onNavigateBack = {
                                        navController.popBackStack()
                                    },
                                    onNavigateToDetail = { localId ->
                                        navController.navigate("gallery_detail/${Uri.encode(localId)}")
                                    },
                                    sharedTransitionScope = this@SharedTransitionLayout,
                                    animatedVisibilityScope = this
                                )
                            }
                            composable("gallery_detail/{localId}") { backStackEntry ->
                                val localId = Uri.decode(backStackEntry.arguments?.getString("localId") ?: "")
                                GalleryDetailScreen(
                                    initialLocalId = localId,
                                    onNavigateBack = {
                                        navController.popBackStack()
                                    },
                                    sharedTransitionScope = this@SharedTransitionLayout,
                                    animatedVisibilityScope = this
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}