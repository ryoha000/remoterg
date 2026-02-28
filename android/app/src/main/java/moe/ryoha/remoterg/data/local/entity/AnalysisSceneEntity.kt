package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "analysis_scene")
data class AnalysisSceneEntity(
    @PrimaryKey
    @ColumnInfo(name = "local_id")
    val localId: String,
    
    @ColumnInfo(name = "location")
    val location: String,
    
    @ColumnInfo(name = "time_of_day")
    val timeOfDay: String,
    
    @ColumnInfo(name = "atmosphere")
    val atmosphere: String
)
