package moe.ryoha.remoterg.data.local

import androidx.room.Database
import androidx.room.RoomDatabase
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import moe.ryoha.remoterg.data.local.dao.AnalysisDao
import moe.ryoha.remoterg.data.local.dao.ScreenshotDao
import moe.ryoha.remoterg.data.local.entity.AnalysisResultEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotFavoriteEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotMapEntity

@Database(
    entities = [
        AnalysisResultEntity::class,
        ScreenshotFavoriteEntity::class,
        ScreenshotMapEntity::class
    ],
    version = 3,
    exportSchema = true
)
abstract class AppDatabase : RoomDatabase() {
    abstract fun screenshotDao(): ScreenshotDao
    abstract fun analysisDao(): AnalysisDao

    companion object {
        /** v1→v2: サムネイルパスカラムを追加 */
        val MIGRATION_1_2 = object : Migration(1, 2) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE screenshot_map ADD COLUMN thumbnail_path TEXT DEFAULT NULL")
            }
        }

        /** v2→v3: プロセスパスカラムを追加 */
        val MIGRATION_2_3 = object : Migration(2, 3) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE screenshot_map ADD COLUMN process_path TEXT DEFAULT NULL")
            }
        }
    }
}
