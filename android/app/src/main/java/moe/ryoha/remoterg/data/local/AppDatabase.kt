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

import moe.ryoha.remoterg.data.local.entity.AnalysisCharacterEntity
import moe.ryoha.remoterg.data.local.entity.AnalysisDialogueEntity
import moe.ryoha.remoterg.data.local.entity.AnalysisSceneEntity

@Database(
    entities = [
        AnalysisResultEntity::class,
        AnalysisSceneEntity::class,
        AnalysisDialogueEntity::class,
        AnalysisCharacterEntity::class,
        ScreenshotFavoriteEntity::class,
        ScreenshotMapEntity::class
    ],
    version = 4,
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

        /** v3→v4: AnalysisResult の正規化 */
        val MIGRATION_3_4 = object : Migration(3, 4) {
            override fun migrate(db: SupportSQLiteDatabase) {
                // 1. 新しいテーブルの作成
                db.execSQL(
                    "CREATE TABLE IF NOT EXISTS `analysis_scene` (`local_id` TEXT NOT NULL, `location` TEXT NOT NULL, `time_of_day` TEXT NOT NULL, `atmosphere` TEXT NOT NULL, PRIMARY KEY(`local_id`))"
                )
                db.execSQL(
                    "CREATE TABLE IF NOT EXISTS `analysis_dialogue` (`local_id` TEXT NOT NULL, `speaker` TEXT NOT NULL, `text` TEXT NOT NULL, PRIMARY KEY(`local_id`))"
                )
                db.execSQL(
                    "CREATE TABLE IF NOT EXISTS `analysis_character` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `local_id` TEXT NOT NULL, `name` TEXT NOT NULL, `expression_tags` TEXT NOT NULL, `visual_description` TEXT NOT NULL, `position` TEXT NOT NULL, FOREIGN KEY(`local_id`) REFERENCES `analysis_results`(`local_id`) ON UPDATE NO ACTION ON DELETE CASCADE )"
                )
                db.execSQL(
                    "CREATE INDEX IF NOT EXISTS `index_analysis_character_local_id` ON `analysis_character` (`local_id`)"
                )

                // 2. 既存の JSON データを移行 (簡易 JSON パースは SQLite では難しいため、起動時にリポジトリ層でマイグレーションを行うか、一旦空にしておくのが安全)
                // 今回は Room の Migration には DDL だけ定義し、データ移行が必要なら Repository 等で行う。
                // ただ分析結果は再取得/再検索できれば良い性質もあるため、既存データはそのままで、新規データから正規化テーブルに入る形でも許容可能。
                // SQLite の json_extract を使って一括で流し込むことも可能ですが、SQLiteのバージョン依存があるため、
                // 必要に応じてアプリケーション起動時に移行処理を走らせます。
                
                // 今回は SQLite > 3.38 で json_extract が標準有効であることを期待して、可能な限り SQL で移行を試みる
                try {
                    // analysis_scene
                    db.execSQL("""
                        INSERT OR IGNORE INTO analysis_scene (local_id, location, time_of_day, atmosphere)
                        SELECT 
                            local_id,
                            COALESCE(json_extract(data, '${'$'}.scene_info.location'), ''),
                            COALESCE(json_extract(data, '${'$'}.scene_info.time_of_day'), ''),
                            COALESCE(json_extract(data, '${'$'}.scene_info.atmosphere'), '')
                        FROM analysis_results
                        WHERE json_extract(data, '${'$'}.scene_info') IS NOT NULL
                    """.trimIndent())

                    // analysis_dialogue
                    db.execSQL("""
                        INSERT OR IGNORE INTO analysis_dialogue (local_id, speaker, text)
                        SELECT 
                            local_id,
                            COALESCE(json_extract(data, '${'$'}.dialogue.speaker'), ''),
                            COALESCE(json_extract(data, '${'$'}.dialogue.text'), '')
                        FROM analysis_results
                        WHERE json_extract(data, '${'$'}.dialogue') IS NOT NULL
                    """.trimIndent())

                    // json_each は Android の SQLite ではデフォルトで無効な場合があるため、
                    // character の移行はスキップするか、アプリケーションレイヤーで対応する。
                    // ここではエラーを無視して続行する。
                } catch (e: Exception) {
                    // ignore JSON extraction failure if not supported
                }
            }
        }
    }
}
