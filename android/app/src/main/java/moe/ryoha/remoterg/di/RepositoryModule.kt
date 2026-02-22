package moe.ryoha.remoterg.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import moe.ryoha.remoterg.data.local.dao.ScreenshotDao
import moe.ryoha.remoterg.data.repository.ScreenshotRepository
import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object RepositoryModule {
    @Provides
    @Singleton
    fun provideScreenshotRepository(
        @ApplicationContext context: Context,
        screenshotDao: ScreenshotDao
    ): ScreenshotRepository {
        return ScreenshotRepository(context, screenshotDao)
    }
}
