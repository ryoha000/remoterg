package moe.ryoha.remoterg.data.local.dao

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import kotlinx.coroutines.flow.Flow
import moe.ryoha.remoterg.data.local.entity.AnalysisResultEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotFavoriteEntity
import moe.ryoha.remoterg.data.local.entity.ScreenshotMapEntity

@Dao
interface ScreenshotDao {

    @Query("SELECT * FROM screenshot_map")
    fun getAllScreenshots(): Flow<List<ScreenshotMapEntity>>

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
}
