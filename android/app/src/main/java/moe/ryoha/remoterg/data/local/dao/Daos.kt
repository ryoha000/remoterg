package moe.ryoha.remoterg.data.local.dao

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import kotlinx.coroutines.flow.Flow
import moe.ryoha.remoterg.data.local.entity.AnalysisCharacterEntity
import moe.ryoha.remoterg.data.local.entity.AnalysisDialogueEntity
import moe.ryoha.remoterg.data.local.entity.AnalysisResultEntity
import moe.ryoha.remoterg.data.local.entity.AnalysisSceneEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotFavoriteEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotMapEntity
import moe.ryoha.remoterg.data.local.entity.GameEntity
import moe.ryoha.remoterg.data.local.entity.GameVndbMapEntity
import androidx.room.Embedded
import androidx.room.ColumnInfo

data class ScreenshotWithGame(
    @Embedded
    val screenshot: ScreenshotMapEntity,
    
    @ColumnInfo(name = "vndb_id")
    val vndbId: String?,
    
    @ColumnInfo(name = "official_title")
    val officialTitle: String?
)

@Dao
interface GameDao {
    @Query("SELECT * FROM game_vndb_maps WHERE vndb_id = :vndbId")
    suspend fun getGameVndbMapByVndbId(vndbId: String): GameVndbMapEntity?

    @Insert(onConflict = OnConflictStrategy.IGNORE)
    suspend fun insertGame(game: GameEntity): Long

    @Insert(onConflict = OnConflictStrategy.IGNORE)
    suspend fun insertGameVndbMap(map: GameVndbMapEntity): Long

    suspend fun upsertGame(vndbId: String, officialTitle: String?): Long {
        val existing = getGameVndbMapByVndbId(vndbId)
        if (existing != null) {
            return existing.gameId
        }
        val gameId = insertGame(GameEntity())
        insertGameVndbMap(GameVndbMapEntity(gameId = gameId, vndbId = vndbId, officialTitle = officialTitle))
        return gameId
    }
}

@Dao
interface ScreenshotDao {

    @Query("SELECT * FROM screenshot_map")
    fun getAllScreenshots(): Flow<List<ScreenshotMapEntity>>

    @Query("""
        SELECT s.*, g.vndb_id, g.official_title 
        FROM screenshot_map s 
        LEFT JOIN game_vndb_maps g ON s.game_id = g.game_id
    """)
    fun getAllScreenshotsWithGame(): Flow<List<ScreenshotWithGame>>

    @Query("SELECT * FROM screenshot_map")
    suspend fun getAllScreenshotsSync(): List<ScreenshotMapEntity>

    @Query("SELECT * FROM screenshot_map WHERE local_id = :localId")
    suspend fun getScreenshotById(localId: String): ScreenshotMapEntity?

    @Query("SELECT * FROM screenshot_map WHERE host_id = :hostId")
    suspend fun getScreenshotsByHostId(hostId: String): List<ScreenshotMapEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertScreenshot(screenshot: ScreenshotMapEntity)

    @Query("DELETE FROM screenshot_map WHERE local_id = :localId")
    suspend fun deleteScreenshot(localId: String)

    @Query("DELETE FROM screenshot_map")
    suspend fun deleteAllScreenshots()

    // Favorites
    @Query("SELECT * FROM screenshot_favorites")
    fun getAllFavorites(): Flow<List<ScreenshotFavoriteEntity>>

    @Insert(onConflict = OnConflictStrategy.IGNORE)
    suspend fun insertFavorite(favorite: ScreenshotFavoriteEntity)

    @Query("DELETE FROM screenshot_favorites WHERE local_id = :localId")
    suspend fun deleteFavorite(localId: String)
    
    @Query("DELETE FROM screenshot_favorites")
    suspend fun deleteAllFavorites()
    
    // Check if favorite
    @Query("SELECT EXISTS(SELECT 1 FROM screenshot_favorites WHERE local_id = :localId)")
    fun isFavorite(localId: String): Flow<Boolean>
}

@Dao
interface AnalysisDao {
    @Query("SELECT * FROM analysis_results WHERE local_id = :localId")
    suspend fun getAnalysisResult(localId: String): AnalysisResultEntity?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertAnalysisResult(result: AnalysisResultEntity)

    @Query("DELETE FROM analysis_results WHERE local_id = :localId")
    suspend fun deleteAnalysisResult(localId: String)

    @Query("SELECT DISTINCT local_id FROM analysis_character WHERE name LIKE '%' || :query || '%'")
    suspend fun searchLocalIdsByCharacter(query: String): List<String>

    @Query("SELECT DISTINCT local_id FROM analysis_dialogue WHERE speaker LIKE '%' || :query || '%'")
    suspend fun searchLocalIdsBySpeaker(query: String): List<String>

    @Query("SELECT local_id FROM analysis_dialogue WHERE text LIKE '%' || :query || '%'")
    suspend fun searchLocalIdsByText(query: String): List<String>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertAnalysisScene(scene: AnalysisSceneEntity)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertAnalysisDialogue(dialogue: AnalysisDialogueEntity)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertAnalysisCharacters(characters: List<AnalysisCharacterEntity>)

    @Query("""
        SELECT name FROM (
            SELECT c.name as name, r.created_at
            FROM analysis_character c
            INNER JOIN analysis_results r ON c.local_id = r.local_id
            WHERE c.name != '' AND c.name IS NOT NULL
        )
        GROUP BY name
        ORDER BY MAX(created_at) DESC
    """)
    fun getRecentCharacters(): Flow<List<String>>

    @Query("""
        SELECT name FROM (
            SELECT d.speaker as name, r.created_at
            FROM analysis_dialogue d
            INNER JOIN analysis_results r ON d.local_id = r.local_id
            WHERE d.speaker != '' AND d.speaker IS NOT NULL
        )
        GROUP BY name
        ORDER BY MAX(created_at) DESC
    """)
    fun getRecentSpeakers(): Flow<List<String>>
}
