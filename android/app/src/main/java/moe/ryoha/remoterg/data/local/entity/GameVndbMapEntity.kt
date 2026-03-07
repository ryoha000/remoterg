package moe.ryoha.remoterg.data.local.entity

import androidx.room.ColumnInfo
import androidx.room.Entity
import androidx.room.ForeignKey
import androidx.room.Index
import androidx.room.PrimaryKey

@Entity(
    tableName = "game_vndb_maps",
    foreignKeys = [
        ForeignKey(
            entity = GameEntity::class,
            parentColumns = ["id"],
            childColumns = ["game_id"],
            onDelete = ForeignKey.CASCADE
        )
    ],
    indices = [
        Index(value = ["vndb_id"], unique = true),
        Index(value = ["game_id"], unique = true)
    ]
)
data class GameVndbMapEntity(
    @PrimaryKey(autoGenerate = true)
    @ColumnInfo(name = "id")
    val id: Long = 0,
    
    @ColumnInfo(name = "game_id")
    val gameId: Long,
    
    @ColumnInfo(name = "vndb_id")
    val vndbId: String,
    
    @ColumnInfo(name = "official_title")
    val officialTitle: String?
)
