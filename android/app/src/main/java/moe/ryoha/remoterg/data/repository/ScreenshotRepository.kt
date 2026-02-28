package moe.ryoha.remoterg.data.repository

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import android.util.Log
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.withContext
import moe.ryoha.remoterg.data.local.dao.ScreenshotDao
import moe.ryoha.remoterg.data.local.entity.ScreenshotMapEntity
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import javax.inject.Inject
import javax.inject.Singleton

data class MediaStoreScreenshot(
    val localId: String,
    val hostId: String,
    val uri: Uri,
    val width: Int,
    val height: Int,
    val dateAdded: Long,
    val windowTitle: String,
    val processName: String,
    val processPath: String? = null,
    val thumbnailPath: String? = null
)

@Singleton
class ScreenshotRepository @Inject constructor(
    @ApplicationContext private val context: Context,
    private val screenshotDao: ScreenshotDao
) {
    fun getAllScreenshots(): Flow<List<ScreenshotMapEntity>> {
        return screenshotDao.getAllScreenshots()
    }

    /**
     * Gets all screenshots from MediaStore and maps them with DB metadata (windowTitle, processName).
     * This ensures MediaStore is the Source of Truth while still utilizing DB metadata.
     */
    fun getAllScreenshotsWithDimensions(): Flow<List<MediaStoreScreenshot>> {
        return screenshotDao.getAllScreenshots().map { dbList ->
            val dbMap = dbList.associateBy { it.localId }
            val mediaStoreItems = mutableListOf<MediaStoreScreenshot>()

            val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
            } else {
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI
            }

            val projection = arrayOf(
                MediaStore.Images.Media._ID,
                MediaStore.Images.Media.WIDTH,
                MediaStore.Images.Media.HEIGHT,
                MediaStore.Images.Media.DATE_ADDED
            )

            // Filter for only our app's screenshots if possible, but for simplicity, we check if they exist in DB
            val sortOrder = "${MediaStore.Images.Media.DATE_ADDED} DESC"

            context.contentResolver.query(
                collection,
                projection,
                null,
                null,
                sortOrder
            )?.use { cursor ->
                val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
                val widthColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.WIDTH)
                val heightColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.HEIGHT)
                val dateAddedColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_ADDED)

                while (cursor.moveToNext()) {
                    val id = cursor.getLong(idColumn)
                    val localId = id.toString()
                    val width = cursor.getInt(widthColumn)
                    val height = cursor.getInt(heightColumn)
                    val dateAdded = cursor.getLong(dateAddedColumn)

                    // Only include images that are in our DB (meaning they were taken by our app)
                    // If we wanted pure MediaStore SoT, we might just show all pictures in the RemoteRG folder.
                    // But combining with DB lets us keep the app logic.
                    val dbEntity = dbMap[localId]
                    if (dbEntity != null) {
                        val contentUri = ContentUris.withAppendedId(collection, id)
                        mediaStoreItems.add(
                            MediaStoreScreenshot(
                                localId = localId,
                                hostId = dbEntity.hostId,
                                uri = contentUri,
                                width = width,
                                height = height,
                                dateAdded = dateAdded,
                                windowTitle = dbEntity.windowTitle,
                                processName = dbEntity.processName,
                                processPath = dbEntity.processPath,
                                thumbnailPath = dbEntity.thumbnailPath
                            )
                        )
                    }
                }
            }
            mediaStoreItems
        }
    }

    suspend fun saveScreenshot(
        hostId: String,
        format: String,
        data: ByteArray,
        windowTitle: String?,
        processPath: String?,
        processName: String?
    ): Uri? = withContext(Dispatchers.IO) {
        try {
            val fileName = "$hostId.$format"
            val mimeType = if (format == "png") "image/png" else "image/jpeg"
            
            val contentValues = ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, fileName)
                put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    put(MediaStore.MediaColumns.RELATIVE_PATH, "Pictures/RemoteRG")
                    put(MediaStore.MediaColumns.IS_PENDING, 1)
                }
            }

            val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            } else {
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI
            }

            val uri = context.contentResolver.insert(collection, contentValues)
            if (uri != null) {
                context.contentResolver.openOutputStream(uri)?.use { out ->
                    out.write(data)
                }

                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    contentValues.clear()
                    contentValues.put(MediaStore.MediaColumns.IS_PENDING, 0)
                    context.contentResolver.update(uri, contentValues, null, null)
                }

                val localId = uri.lastPathSegment ?: uri.toString()
                
                // サムネイルを事前生成してギャラリーでのフルデコードを回避
                val thumbnailPath = generateThumbnail(data, localId)

                // DB にメタデータとサムネイルパスを保存
                screenshotDao.insertScreenshot(
                    ScreenshotMapEntity(
                        localId = localId,
                        hostId = hostId,
                        windowTitle = windowTitle ?: "",
                        processName = processName ?: "",
                        processPath = processPath,
                        thumbnailPath = thumbnailPath
                    )
                )

                Log.d(TAG, "Screenshot saved to MediaStore and DB. URI: $uri, LocalID: $localId")
                return@withContext uri
            } else {
                Log.e(TAG, "Failed to create MediaStore entry")
                return@withContext null
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error saving screenshot", e)
            return@withContext null
        }
    }

    suspend fun saveLocalScreenshot(
        bitmap: Bitmap,
        hostId: String = "local_${System.currentTimeMillis()}",
        windowTitle: String? = "Client Screenshot",
        processPath: String? = "remoterg/client",
        processName: String? = "Android Client"
    ): Uri? = withContext(Dispatchers.IO) {
        val stream = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, 100, stream)
        val data = stream.toByteArray()

        saveScreenshot(
            hostId = hostId,
            format = "jpeg",
            data = data,
            windowTitle = windowTitle,
            processPath = processPath,
            processName = processName
        )
    }

    /**
     * 単体スクリーンショットを MediaStore と DB から削除する
     */
    suspend fun deleteScreenshot(localId: String): Boolean = withContext(Dispatchers.IO) {
        try {
            val id = localId.toLongOrNull()
            if (id != null) {
                val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
                } else {
                    MediaStore.Images.Media.EXTERNAL_CONTENT_URI
                }
                val uri = ContentUris.withAppendedId(collection, id)
                try {
                    context.contentResolver.delete(uri, null, null)
                    Log.d(TAG, "MediaStore からファイル削除成功: $localId")
                } catch (e: Exception) {
                    Log.w(TAG, "MediaStore からのファイル削除に失敗 (スキップ): $localId", e)
                }
            }
            // サムネイルファイルを削除
            deleteThumbnail(localId)
            // DB レコードを削除
            screenshotDao.deleteScreenshot(localId)
            screenshotDao.deleteFavorite(localId)
            true
        } catch (e: Exception) {
            Log.e(TAG, "スクリーンショット削除エラー: $localId", e)
            false
        }
    }

    /**
     * 全スクリーンショットを MediaStore と DB から削除する。
     * DB に記録された localId を使い個別に MediaStore から削除する方式。
     */
    suspend fun clearAllScreenshots(): Boolean = withContext(Dispatchers.IO) {
        try {
            // DB に記録されたスクリーンショットを個別に MediaStore から削除
            val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
            } else {
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI
            }
            val dbItems = screenshotDao.getAllScreenshotsSync()
            var deletedCount = 0

            for (item in dbItems) {
                try {
                    val id = item.localId.toLongOrNull()
                    if (id != null) {
                        val uri = ContentUris.withAppendedId(collection, id)
                        val result = context.contentResolver.delete(uri, null, null)
                        if (result > 0) deletedCount++
                    }
                } catch (e: Exception) {
                    Log.w(TAG, "MediaStore からのファイル削除に失敗 (スキップ): ${item.localId}", e)
                }
            }
            Log.d(TAG, "MediaStore から $deletedCount/${dbItems.size} 件のファイルを削除")

            // サムネイルディレクトリを全削除
            clearThumbnailDir()
            // DB レコードを全削除
            screenshotDao.deleteAllScreenshots()
            screenshotDao.deleteAllFavorites()

            true
        } catch (e: Exception) {
            Log.e(TAG, "全スクリーンショット削除エラー", e)
            false
        }
    }

    /**
     * デバッグ用: サムネイル未生成の全スクリーンショットに対してサムネイルを一括生成する。
     * MediaStore からフル画像を読み取り、リサイズ済み JPEG を内部ストレージに保存。
     */
    suspend fun generateAllThumbnails(): Int = withContext(Dispatchers.IO) {
        var generated = 0
        try {
            val dbItems = screenshotDao.getAllScreenshotsSync()
            val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
            } else {
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI
            }

            for (item in dbItems) {
                // 既にサムネイルがあるならスキップ
                if (item.thumbnailPath != null && File(item.thumbnailPath).exists()) continue

                try {
                    val id = item.localId.toLongOrNull() ?: continue
                    val uri = ContentUris.withAppendedId(collection, id)

                    // MediaStore からフル画像を読み取る
                    val data = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                    if (data == null || data.isEmpty()) {
                        Log.w(TAG, "画像データ読み取り失敗: ${item.localId}")
                        continue
                    }

                    val thumbnailPath = generateThumbnail(data, item.localId)
                    if (thumbnailPath != null) {
                        // DB のサムネイルパスを更新
                        screenshotDao.insertScreenshot(
                            item.copy(thumbnailPath = thumbnailPath)
                        )
                        generated++
                    }
                } catch (e: Exception) {
                    Log.w(TAG, "サムネイル生成失敗: ${item.localId}", e)
                }
            }
            Log.d(TAG, "サムネイル一括生成完了: $generated/${dbItems.size}")
        } catch (e: Exception) {
            Log.e(TAG, "サムネイル一括生成エラー", e)
        }
        generated
    }

    // ── サムネイル生成・管理 ──────────────────────────────────

    /** サムネイル保存先の高さ (ギャラリーの TARGET_ROW_HEIGHT 180dp × 2x density) */
    private companion object {
        const val TAG = "ScreenshotRepository"
        const val THUMBNAIL_HEIGHT = 360
        const val THUMBNAIL_QUALITY = 80
        const val THUMBNAIL_DIR = "thumbnails"
    }

    private fun getThumbnailDir(): File {
        return File(context.filesDir, THUMBNAIL_DIR).also { it.mkdirs() }
    }

    /**
     * フル画像のバイト配列からサムネイルを生成し、内部ストレージに JPEG で保存。
     * 既にメモリに存在する data を使うため、追加の I/O は発生しない。
     */
    private fun generateThumbnail(data: ByteArray, localId: String): String? {
        return try {
            // まずサイズだけ取得して縮小率を計算
            val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(data, 0, data.size, options)
            val origWidth = options.outWidth
            val origHeight = options.outHeight
            if (origWidth <= 0 || origHeight <= 0) return null

            // inSampleSize で粗くデコードしてメモリ節約
            val sampleSize = (origHeight / THUMBNAIL_HEIGHT).coerceAtLeast(1)
            val decodeOptions = BitmapFactory.Options().apply { inSampleSize = sampleSize }
            val sampled = BitmapFactory.decodeByteArray(data, 0, data.size, decodeOptions) ?: return null

            // 最終リサイズ
            val scale = THUMBNAIL_HEIGHT.toFloat() / sampled.height
            val thumbWidth = (sampled.width * scale).toInt()
            val thumbnail = Bitmap.createScaledBitmap(sampled, thumbWidth, THUMBNAIL_HEIGHT, true)
            if (thumbnail != sampled) sampled.recycle()

            val file = File(getThumbnailDir(), "$localId.jpg")
            FileOutputStream(file).use { out ->
                thumbnail.compress(Bitmap.CompressFormat.JPEG, THUMBNAIL_QUALITY, out)
            }
            thumbnail.recycle()

            Log.d(TAG, "サムネイル生成: ${file.absolutePath} (${thumbWidth}x$THUMBNAIL_HEIGHT)")
            file.absolutePath
        } catch (e: Exception) {
            Log.w(TAG, "サムネイル生成失敗 (フォールバック使用): $localId", e)
            null
        }
    }

    private fun deleteThumbnail(localId: String) {
        try {
            File(getThumbnailDir(), "$localId.jpg").delete()
        } catch (e: Exception) {
            Log.w(TAG, "サムネイル削除失敗: $localId", e)
        }
    }

    private fun clearThumbnailDir() {
        try {
            getThumbnailDir().listFiles()?.forEach { it.delete() }
        } catch (e: Exception) {
            Log.w(TAG, "サムネイルディレクトリクリア失敗", e)
        }
    }
}
