package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.ForeignKey
import androidx.room.Index
import androidx.room.PrimaryKey

@Entity(
    tableName = "analysis_character",
    foreignKeys = [
        ForeignKey(
            entity = AnalysisResultEntity::class,
            parentColumns = ["local_id"],
            childColumns = ["local_id"],
            onDelete = ForeignKey.CASCADE
        )
    ],
    indices = [
        Index(value = ["local_id"])
    ]
)
data class AnalysisCharacterEntity(
    @PrimaryKey(autoGenerate = true)
    @ColumnInfo(name = "id")
    val id: Long = 0,

    @ColumnInfo(name = "local_id")
    val localId: String,
    
    @ColumnInfo(name = "name")
    val name: String,
    
    @ColumnInfo(name = "expression_tags")
    val expressionTags: String, // comma-separated
    
    @ColumnInfo(name = "visual_description")
    val visualDescription: String,
    
    @ColumnInfo(name = "position")
    val position: String
)
