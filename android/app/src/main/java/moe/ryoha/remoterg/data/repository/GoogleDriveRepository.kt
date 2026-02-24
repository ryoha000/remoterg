package moe.ryoha.remoterg.data.repository

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import dagger.hilt.android.qualifiers.ApplicationContext
import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.engine.okhttp.OkHttp
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsText
import io.ktor.client.request.forms.MultiPartFormDataContent
import io.ktor.client.request.forms.formData
import io.ktor.client.request.header
import io.ktor.http.Headers
import io.ktor.http.HttpHeaders
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.contentOrNull
import io.ktor.http.ContentType
import io.ktor.http.contentType
import io.ktor.serialization.kotlinx.json.json
import javax.inject.Inject
import javax.inject.Singleton

@Serializable
data class AuthUrlResponse(
    val url: String
)

@Serializable
data class TokenResponse(
    val access_token: String,
    val expires_in: Int,
    val refresh_token: String? = null,
    val scope: String,
    val token_type: String
)


@Singleton
class GoogleDriveRepository @Inject constructor(
    @ApplicationContext private val context: Context
) {
    private val prefs: SharedPreferences = context.getSharedPreferences("GoogleDrivePrefs", Context.MODE_PRIVATE)

    private val _isConnected = MutableStateFlow(hasAccessToken())
    val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

    val authCodeFlow = MutableSharedFlow<String>(extraBufferCapacity = 1)

    private val httpClient = HttpClient(OkHttp) {
        install(ContentNegotiation) {
            json(Json {
                ignoreUnknownKeys = true
                prettyPrint = true
                isLenient = true
            })
        }
    }

    private fun hasAccessToken(): Boolean {
        return prefs.getString("access_token", null) != null
    }

    fun getAccessToken(): String? {
        return prefs.getString("access_token", null)
    }

    suspend fun fetchAuthUrl(signalingUrl: String): String {
        val baseUrl = signalingUrl.replace("ws://", "http://").replace("wss://", "https://")
        val url = baseUrl.substringBefore("/api/signal") + "/api/drive/auth-url"
        return try {
            val response: AuthUrlResponse = httpClient.get(url).body()
            response.url
        } catch (e: Exception) {
            Log.e("GoogleDriveRepo", "Failed to fetch auth URL", e)
            throw e
        }
    }

    suspend fun exchangeCodeToTokens(signalingUrl: String, code: String) {
        val baseUrl = signalingUrl.replace("ws://", "http://").replace("wss://", "https://")
        val url = baseUrl.substringBefore("/api/signal") + "/api/drive/token"
        try {
            val response: TokenResponse = httpClient.post(url) {
                contentType(ContentType.Application.Json)
                setBody(mapOf("code" to code))
            }.body()

            prefs.edit().apply {
                putString("access_token", response.access_token)
                if (response.refresh_token != null) {
                    putString("refresh_token", response.refresh_token)
                }
                putLong("expires_at", System.currentTimeMillis() + response.expires_in * 1000L)
                apply()
            }
            _isConnected.value = true
        } catch (e: Exception) {
            Log.e("GoogleDriveRepo", "Failed to exchange code", e)
            throw e
        }
    }

    private suspend fun getOrCreateFolder(folderName: String, token: String): String? {
        try {
            // Search for existing folder
            val query = "mimeType='application/vnd.google-apps.folder' and name='$folderName' and trashed=false"
            val searchResponse = httpClient.get("https://www.googleapis.com/drive/v3/files") {
                header(HttpHeaders.Authorization, "Bearer $token")
                url {
                    parameters.append("q", query)
                    parameters.append("fields", "files(id, name)")
                }
            }
            if (searchResponse.status.value in 200..299) {
                val bodyText = searchResponse.bodyAsText()
                val jsonEl = Json.parseToJsonElement(bodyText).jsonObject
                val files = jsonEl["files"]?.jsonArray
                if (!files.isNullOrEmpty()) {
                    val folderId = files[0].jsonObject["id"]?.jsonPrimitive?.contentOrNull
                    if (folderId != null) return folderId
                }
            } else {
                Log.e("GoogleDriveRepo", "Folder search failed: ${searchResponse.status} - ${searchResponse.bodyAsText()}")
            }

            // Folder not found, create it
            val folderMetadata = buildJsonObject {
                put("name", folderName)
                put("mimeType", "application/vnd.google-apps.folder")
            }
            val createResponse = httpClient.post("https://www.googleapis.com/drive/v3/files") {
                header(HttpHeaders.Authorization, "Bearer $token")
                contentType(ContentType.Application.Json)
                setBody(folderMetadata.toString())
            }
            if (createResponse.status.value in 200..299) {
                val createBodyText = createResponse.bodyAsText()
                val createdJsonEl = Json.parseToJsonElement(createBodyText).jsonObject
                return createdJsonEl["id"]?.jsonPrimitive?.contentOrNull
            } else {
                Log.e("GoogleDriveRepo", "Folder creation failed: ${createResponse.status} - ${createResponse.bodyAsText()}")
            }
        } catch (e: Exception) {
            Log.e("GoogleDriveRepo", "Exception during folder retrieval/creation", e)
        }
        return null
    }

    suspend fun uploadToDrive(
        data: ByteArray,
        format: String,
        windowTitle: String?,
        processName: String?
    ) {
        val token = getAccessToken() ?: throw IllegalStateException("Not connected to Google Drive")
        val mimeType = if (format.lowercase() == "jpg" || format.lowercase() == "jpeg") "image/jpeg" else "image/png"
        val fileName = "Screenshot_${System.currentTimeMillis()}.${format.lowercase()}"
        
        val metadata = buildJsonObject {
            put("name", fileName)
            val descriptionParts = mutableListOf<String>()
            if (processName != null) descriptionParts.add("Process: $processName")
            if (windowTitle != null) descriptionParts.add("Window: $windowTitle")
            if (descriptionParts.isNotEmpty()) {
                put("description", descriptionParts.joinToString("\n"))
            }

            // Attempt to put the file into the Remoterg folder
            val folderId = getOrCreateFolder("Remoterg", token)
            if (folderId != null) {
                put("parents", kotlinx.serialization.json.buildJsonArray {
                    add(kotlinx.serialization.json.JsonPrimitive(folderId))
                })
            }
        }

        try {
            val response = httpClient.post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart") {
                header(HttpHeaders.Authorization, "Bearer $token")
                setBody(
                    MultiPartFormDataContent(
                        formData {
                            append("metadata", metadata.toString(), Headers.build {
                                append(HttpHeaders.ContentType, "application/json; charset=UTF-8")
                            })
                            append("file", data, Headers.build {
                                append(HttpHeaders.ContentType, mimeType)
                            })
                        }
                    )
                )
            }
            if (response.status.value !in 200..299) {
                Log.e("GoogleDriveRepo", "Upload failed: ${response.status} - ${response.bodyAsText()}")
            } else {
                Log.d("GoogleDriveRepo", "Upload successful: ${response.bodyAsText()}")
            }
        } catch (e: Exception) {
            Log.e("GoogleDriveRepo", "Exception during upload", e)
        }
    }

    fun disconnect() {
        prefs.edit().clear().apply()
        _isConnected.value = false
    }
}
