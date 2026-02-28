package moe.ryoha.remoterg.di

import android.content.Context
import androidx.room.Room
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import moe.ryoha.remoterg.data.local.AppDatabase
import moe.ryoha.remoterg.data.local.dao.AnalysisDao
import moe.ryoha.remoterg.data.local.dao.ScreenshotDao
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object DatabaseModule {

    @Provides
    @Singleton
    fun provideAppDatabase(@ApplicationContext context: Context): AppDatabase {
        return Room.databaseBuilder(
            context,
            AppDatabase::class.java,
            "remoterg_db"
        )
            .addMigrations(AppDatabase.MIGRATION_1_2, AppDatabase.MIGRATION_2_3, AppDatabase.MIGRATION_3_4)
            .build()
    }

    @Provides
    fun provideScreenshotDao(database: AppDatabase): ScreenshotDao {
        return database.screenshotDao()
    }

    @Provides
    fun provideAnalysisDao(database: AppDatabase): AnalysisDao {
        return database.analysisDao()
    }
}
