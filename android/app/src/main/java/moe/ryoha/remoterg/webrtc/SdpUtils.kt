package moe.ryoha.remoterg.webrtc

/**
 * WebRTC SDP (Session Description Protocol) 操作ユーティリティ
 */
object SdpUtils {

    /**
     * SDP文字列内の `m=video` 行に含まれるペイロードタイプを並べ替え、
     * 指定されたコーデック（例: "H264", "VP8", "VP9", "AV1"）の優先順位を最も高く（先頭に）する。
     * 
     * @param sdp 元の SDP 文字列
     * @param codec 優先したいコーデック（大文字・小文字は無視してマッチング）
     * @return 優先コーデックのペイロードタイプが前に来た新しい SDP 文字列
     */
    fun preferCodecSdp(sdp: String, codec: String): String {
        val lines = sdp.split("\r\n".toRegex()).toMutableList()
        if (lines.isEmpty() || (lines.size == 1 && lines[0].isEmpty())) {
            lines.clear()
            lines.addAll(sdp.split("\n"))
        }

        var mLineIndex = -1
        val codecPayloadTypes = mutableListOf<String>()

        // 1. 各コーデックの a=rtpmap 行を走査し、優先したいコーデックに該当するペイロードタイプを検索
        val codecLower = codec.lowercase()
        val regex = Regex("^a=rtpmap:(\\d+) (\\w+)/\\d+")

        for (i in lines.indices) {
            val line = lines[i]
            if (line.startsWith("m=video ")) {
                mLineIndex = i
            } else if (line.startsWith("a=rtpmap:")) {
                val matchResult = regex.find(line)
                if (matchResult != null) {
                    val payloadType = matchResult.groupValues[1]
                    val codecName = matchResult.groupValues[2].lowercase()
                    // 指定されたコーデック名と一致するか検証（H264, VP8 等）
                    if (codecName == codecLower) {
                        codecPayloadTypes.add(payloadType)
                    }
                }
            }
        }

        // 該当コーデックが見つからない、または video の m=行 がない場合は何もしない
        if (mLineIndex == -1 || codecPayloadTypes.isEmpty()) {
            return sdp
        }

        // 2. m=video 行のペイロードタイプのリストを取得し、優先コーデックのものを先頭に移動
        val mLine = lines[mLineIndex]
        val parts = mLine.split(" ").toMutableList()
        // m=video <port> <proto> <fmt1> <fmt2> ...
        if (parts.size > 3) {
            val originalPayloadTypes = parts.subList(3, parts.size)
            val newPayloadTypes = mutableListOf<String>()

            // 優先すべきペイロードタイプを先に追加
            for (pt in codecPayloadTypes) {
                if (originalPayloadTypes.contains(pt)) {
                    newPayloadTypes.add(pt)
                }
            }
            // 残りのペイロードタイプを追加
            for (pt in originalPayloadTypes) {
                if (!newPayloadTypes.contains(pt)) {
                    newPayloadTypes.add(pt)
                }
            }

            // 新しい m=video 行を構築
            lines[mLineIndex] = "${parts[0]} ${parts[1]} ${parts[2]} ${newPayloadTypes.joinToString(" ")}"
        }

        // リストを改行で結合して返す
        return lines.joinToString("\r\n")
    }
}
