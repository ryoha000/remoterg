package moe.ryoha.remoterg.ui.util

import moe.ryoha.remoterg.data.repository.MediaStoreScreenshot
import moe.ryoha.remoterg.ui.viewmodel.DateSection
import moe.ryoha.remoterg.ui.viewmodel.JustifiedItem
import moe.ryoha.remoterg.ui.viewmodel.JustifiedRow

/**
 * Justified レイアウト（日付セクション + 行分割）の計算を行う純関数ユーティリティ。
 *
 * GalleryViewModel からの責務分離により、ユニットテストが容易になる。
 * 入力: スクリーンショットリスト + 画面幅 → 出力: DateSection リスト
 */
object JustifiedLayoutCalculator {

    private const val TARGET_ROW_HEIGHT = 180f
    private const val SPACING = 4f

    /**
     * スクリーンショットを日付ごとにグルーピングし、Justified レイアウトの行に分割する。
     *
     * @param screenshots スクリーンショットの一覧
     * @param favoriteIds お気に入りスクリーンショットの localId セット
     * @param screenWidthDp 画面幅 (dp)
     * @return 日付セクションのリスト（新しい日付が先頭）
     */
    fun calculateSections(
        screenshots: List<MediaStoreScreenshot>,
        favoriteIds: Set<String>,
        screenWidthDp: Float
    ): List<DateSection> {
        if (screenWidthDp <= 0f || screenshots.isEmpty()) return emptyList()

        val containerWidthDp = screenWidthDp - (SPACING * 2)
        val sections = mutableListOf<DateSection>()

        // 日付ごとにグルーピング
        val cal = java.util.Calendar.getInstance()
        val groupedByDate = screenshots.groupBy { screenshot ->
            cal.timeInMillis = screenshot.dateAdded * 1000L
            cal.set(java.util.Calendar.HOUR_OF_DAY, 0)
            cal.set(java.util.Calendar.MINUTE, 0)
            cal.set(java.util.Calendar.SECOND, 0)
            cal.set(java.util.Calendar.MILLISECOND, 0)
            cal.timeInMillis
        }

        val sortedDates = groupedByDate.keys.sortedDescending()

        for (date in sortedDates) {
            val dateScreenshots = groupedByDate[date] ?: continue
            val rows = buildJustifiedRows(dateScreenshots, favoriteIds, containerWidthDp)
            sections.add(DateSection(formatDateHeader(date), rows))
        }

        return sections
    }

    /**
     * 1 日分のスクリーンショットを Justified 行に分割する。
     */
    private fun buildJustifiedRows(
        screenshots: List<MediaStoreScreenshot>,
        favoriteIds: Set<String>,
        containerWidthDp: Float
    ): List<JustifiedRow> {
        val rows = mutableListOf<JustifiedRow>()
        var currentRow = mutableListOf<Pair<MediaStoreScreenshot, Float>>()
        var currentRowAspectRatio = 0f

        for (screenshot in screenshots) {
            val aspectRatio = if (screenshot.width > 0 && screenshot.height > 0) {
                screenshot.width.toFloat() / screenshot.height.toFloat()
            } else {
                16f / 9f
            }

            currentRow.add(screenshot to aspectRatio)
            currentRowAspectRatio += aspectRatio

            val estimatedWidth = currentRowAspectRatio * TARGET_ROW_HEIGHT

            if (estimatedWidth >= containerWidthDp) {
                val spacingTotal = (currentRow.size - 1) * SPACING
                val rowHeight = (containerWidthDp - spacingTotal) / currentRowAspectRatio

                rows.add(JustifiedRow(
                    items = currentRow.map { (ss, ar) ->
                        JustifiedItem(ss, favoriteIds.contains(ss.localId), ar, ar * rowHeight)
                    },
                    isLastRow = false,
                    rowHeightDp = rowHeight
                ))
                currentRow = mutableListOf()
                currentRowAspectRatio = 0f
            }
        }

        // 最終行（コンテナ幅に満たない場合）
        if (currentRow.isNotEmpty()) {
            rows.add(JustifiedRow(
                items = currentRow.map { (ss, ar) ->
                    JustifiedItem(ss, favoriteIds.contains(ss.localId), ar, ar * TARGET_ROW_HEIGHT)
                },
                isLastRow = true,
                rowHeightDp = TARGET_ROW_HEIGHT
            ))
        }

        return rows
    }

    /**
     * タイムスタンプ (ms) を日付ヘッダー文字列に変換する。
     * 今日 / 昨日 / yyyy年M月d日 の形式。
     */
    internal fun formatDateHeader(timestampMs: Long): String {
        val cal = java.util.Calendar.getInstance()
        cal.set(java.util.Calendar.HOUR_OF_DAY, 0)
        cal.set(java.util.Calendar.MINUTE, 0)
        cal.set(java.util.Calendar.SECOND, 0)
        cal.set(java.util.Calendar.MILLISECOND, 0)
        val todayStart = cal.timeInMillis

        cal.add(java.util.Calendar.DAY_OF_YEAR, -1)
        val yesterdayStart = cal.timeInMillis

        val targetCal = java.util.Calendar.getInstance()
        targetCal.timeInMillis = timestampMs
        targetCal.set(java.util.Calendar.HOUR_OF_DAY, 0)
        targetCal.set(java.util.Calendar.MINUTE, 0)
        targetCal.set(java.util.Calendar.SECOND, 0)
        targetCal.set(java.util.Calendar.MILLISECOND, 0)
        val targetStart = targetCal.timeInMillis

        if (targetStart == todayStart) return "今日"
        if (targetStart == yesterdayStart) return "昨日"

        val format = java.text.SimpleDateFormat("yyyy年M月d日", java.util.Locale.US)
        return format.format(java.util.Date(timestampMs))
    }
}
