package moe.ryoha.remoterg.ui.util

import android.net.Uri
import io.mockk.mockk
import moe.ryoha.remoterg.data.repository.MediaStoreScreenshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JustifiedLayoutCalculator のユニットテスト
 *
 * 純関数のため、テスト容易。Uri は mockk でモック化。
 */
class JustifiedLayoutCalculatorTest {

    // テスト用のスクリーンショットファクトリ
    private var idCounter = 0
    private fun makeScreenshot(
        localId: String = "id_${idCounter++}",
        width: Int = 1920,
        height: Int = 1080,
        dateAdded: Long = System.currentTimeMillis() / 1000
    ) = MediaStoreScreenshot(
        localId = localId,
        hostId = "host_1",
        uri = mockk<Uri>(relaxed = true),
        width = width,
        height = height,
        dateAdded = dateAdded,
        windowTitle = "Test Window",
        processName = "test.exe"
    )

    @Test
    fun `空リストの場合は空を返す`() {
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = emptyList(),
            favoriteIds = emptySet(),
            screenWidthDp = 400f
        )
        assertTrue(result.isEmpty())
    }

    @Test
    fun `画面幅が0以下の場合は空を返す`() {
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = listOf(makeScreenshot()),
            favoriteIds = emptySet(),
            screenWidthDp = 0f
        )
        assertTrue(result.isEmpty())
    }

    @Test
    fun `1つのスクリーンショットは1セクション1行になる`() {
        val screenshots = listOf(makeScreenshot())
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = screenshots,
            favoriteIds = emptySet(),
            screenWidthDp = 400f
        )
        assertEquals(1, result.size)
        assertEquals(1, result[0].rows.size)
        assertTrue(result[0].rows[0].isLastRow)
        assertEquals(1, result[0].rows[0].items.size)
    }

    @Test
    fun `お気に入り状態が正しく反映される`() {
        val ss = makeScreenshot(localId = "fav_id")
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = listOf(ss),
            favoriteIds = setOf("fav_id"),
            screenWidthDp = 400f
        )
        assertTrue(result[0].rows[0].items[0].isFavorite)
    }

    @Test
    fun `お気に入りでない場合は false`() {
        val ss = makeScreenshot(localId = "not_fav")
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = listOf(ss),
            favoriteIds = setOf("other_id"),
            screenWidthDp = 400f
        )
        assertEquals(false, result[0].rows[0].items[0].isFavorite)
    }

    @Test
    fun `複数のスクリーンショットが同一日付で1セクションにグルーピングされる`() {
        val today = System.currentTimeMillis() / 1000
        val screenshots = (1..5).map {
            makeScreenshot(localId = "id_group_$it", dateAdded = today)
        }
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = screenshots,
            favoriteIds = emptySet(),
            screenWidthDp = 400f
        )
        assertEquals(1, result.size)
        val totalItems = result[0].rows.sumOf { it.items.size }
        assertEquals(5, totalItems)
    }

    @Test
    fun `異なる日付のスクリーンショットが別セクションに分かれる`() {
        val today = System.currentTimeMillis() / 1000
        val yesterday = today - 86400
        val screenshots = listOf(
            makeScreenshot(localId = "today", dateAdded = today),
            makeScreenshot(localId = "yesterday", dateAdded = yesterday)
        )
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = screenshots,
            favoriteIds = emptySet(),
            screenWidthDp = 400f
        )
        assertEquals(2, result.size)
    }

    @Test
    fun `日付が新しい順にソートされる`() {
        val today = System.currentTimeMillis() / 1000
        val yesterday = today - 86400
        val screenshots = listOf(
            makeScreenshot(localId = "yesterday_item", dateAdded = yesterday),
            makeScreenshot(localId = "today_item", dateAdded = today)
        )
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = screenshots,
            favoriteIds = emptySet(),
            screenWidthDp = 400f
        )
        // 最初のセクションが今日であること
        assertEquals("today_item", result[0].rows[0].items[0].screenshot.localId)
    }

    @Test
    fun `幅が十分小さいと複数行に分割される`() {
        val today = System.currentTimeMillis() / 1000
        val screenshots = (1..5).map {
            makeScreenshot(localId = "narrow_$it", width = 1920, height = 1080, dateAdded = today)
        }
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = screenshots,
            favoriteIds = emptySet(),
            screenWidthDp = 200f
        )
        val hasNonLastRow = result[0].rows.any { !it.isLastRow }
        assertTrue("幅が狭い画面では複数行に分割されるべき", hasNonLastRow)
    }

    @Test
    fun `アスペクト比が0x0の場合デフォルト 16対9 が使われる`() {
        val ss = makeScreenshot(width = 0, height = 0)
        val result = JustifiedLayoutCalculator.calculateSections(
            screenshots = listOf(ss),
            favoriteIds = emptySet(),
            screenWidthDp = 400f
        )
        val item = result[0].rows[0].items[0]
        assertEquals(16f / 9f, item.aspectRatio, 0.01f)
    }

    @Test
    fun `formatDateHeader が今日の場合は今日を返す`() {
        val todayCal = java.util.Calendar.getInstance()
        todayCal.set(java.util.Calendar.HOUR_OF_DAY, 0)
        todayCal.set(java.util.Calendar.MINUTE, 0)
        todayCal.set(java.util.Calendar.SECOND, 0)
        todayCal.set(java.util.Calendar.MILLISECOND, 0)
        val result = JustifiedLayoutCalculator.formatDateHeader(todayCal.timeInMillis)
        assertEquals("今日", result)
    }

    @Test
    fun `formatDateHeader が昨日の場合は昨日を返す`() {
        val cal = java.util.Calendar.getInstance()
        cal.set(java.util.Calendar.HOUR_OF_DAY, 0)
        cal.set(java.util.Calendar.MINUTE, 0)
        cal.set(java.util.Calendar.SECOND, 0)
        cal.set(java.util.Calendar.MILLISECOND, 0)
        cal.add(java.util.Calendar.DAY_OF_YEAR, -1)
        val result = JustifiedLayoutCalculator.formatDateHeader(cal.timeInMillis)
        assertEquals("昨日", result)
    }
}
