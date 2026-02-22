package moe.ryoha.remoterg.di

import android.content.Context
import coil.ImageLoader
import coil.disk.DiskCache
import coil.memory.MemoryCache
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import kotlinx.coroutines.Dispatchers
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object CoilModule {
    @Provides
    @Singleton
    fun provideImageLoader(@ApplicationContext context: Context): ImageLoader {
        return ImageLoader.Builder(context)
            // 同時デコード数を制限し、DefaultDispatcher の専有を防ぐ
            .dispatcher(Dispatchers.IO.limitedParallelism(3))
            // クロスフェードを無効化: 2枚同時にBitmapを保持するGPU転送コストを回避
            .crossfade(false)
            // ディスクキャッシュ: ダウンサンプリング済み画像を確実にキャッシュ
            .diskCache {
                DiskCache.Builder()
                    .directory(context.cacheDir.resolve("coil_cache"))
                    .maxSizePercent(0.05) // ストレージの5%（デフォルト2%）
                    .build()
            }
            // メモリキャッシュ: ギャラリー画面のスクロール体験向上
            .memoryCache {
                MemoryCache.Builder(context)
                    .maxSizePercent(0.30) // アプリメモリの30%（デフォルト25%）
                    .build()
            }
            .build()
    }
}
