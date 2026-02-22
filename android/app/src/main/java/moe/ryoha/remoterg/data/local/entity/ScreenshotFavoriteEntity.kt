package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "screenshot_favorites")
data class ScreenshotFavoriteEntity(
    @PrimaryKey
    @ColumnInfo(name = "local_id")
    val localId: String
)
