package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "analysis_results")
data class AnalysisResultEntity(
    @PrimaryKey
    @ColumnInfo(name = "local_id")
    val localId: String,
    
    @ColumnInfo(name = "data")
    val data: String,
    
    @ColumnInfo(name = "created_at")
    val createdAt: Long
)
