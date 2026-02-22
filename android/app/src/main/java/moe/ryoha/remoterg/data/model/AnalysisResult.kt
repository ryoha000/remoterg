package moe.ryoha.remoterg.data.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class SceneInfo(
    val location: String = "",
    @SerialName("time_of_day") val timeOfDay: String = "",
    val atmosphere: String = ""
)

@Serializable
data class Dialogue(
    val speaker: String = "",
    val text: String = ""
)

@Serializable
data class Character(
    val name: String = "",
    @SerialName("expression_tags") val expressionTags: List<String> = emptyList(),
    @SerialName("visual_description") val visualDescription: String = "",
    val position: String = ""
)

@Serializable
data class AnalysisResult(
    @SerialName("scene_info") val sceneInfo: SceneInfo? = null,
    val dialogue: Dialogue? = null,
    val characters: List<Character> = emptyList()
)
