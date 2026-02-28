package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "screenshot_map")
data class ScreenshotMapEntity(
    @PrimaryKey
    @ColumnInfo(name = "local_id")
    val localId: String,

    @ColumnInfo(name = "host_id")
    val hostId: String,

    @ColumnInfo(name = "window_title", defaultValue = "")
    val windowTitle: String = "",

    @ColumnInfo(name = "process_name", defaultValue = "")
    val processName: String = "",

    @ColumnInfo(name = "process_path", defaultValue = "NULL")
    val processPath: String? = null,

    @ColumnInfo(name = "thumbnail_path", defaultValue = "NULL")
    val thumbnailPath: String? = null
)
