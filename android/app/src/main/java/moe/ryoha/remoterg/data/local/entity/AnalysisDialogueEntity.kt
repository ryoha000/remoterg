package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "analysis_dialogue")
data class AnalysisDialogueEntity(
    @PrimaryKey
    @ColumnInfo(name = "local_id")
    val localId: String,
    
    @ColumnInfo(name = "speaker")
    val speaker: String,
    
    @ColumnInfo(name = "text")
    val text: String
)
