package moe.ryoha.remoterg.webrtc

/**
 * NTP タイムスタンプの変換ユーティリティ。
 * abs-capture-time RTP 拡張で送られる UQ32.32 形式の NTP タイムスタンプを
 * Unix ミリ秒に変換する。
 */
object NtpUtils {

    /** NTP epoch (1900-01-01) と Unix epoch (1970-01-01) の秒差 */
    private const val NTP_EPOCH_OFFSET_SECS = 2_208_988_800L

    /**
     * UQ32.32 形式の NTP タイムスタンプを Unix ミリ秒に変換する。
     * @param ntp 64bit NTP タイムスタンプ（上位32bit: 秒、下位32bit: 小数部）
     * @return Unix epoch からのミリ秒
     */
    fun ntpU64ToUnixMs(ntp: Long): Long {
        val ntpSecs = ntp.ushr(32)
        val unixSecs = ntpSecs - NTP_EPOCH_OFFSET_SECS
        val fracMs = ((ntp and 0xFFFFFFFFL) * 1000L).ushr(32)
        return unixSecs * 1000L + fracMs
    }

    /**
     * EncodedImage.captureTimeNs を Unix ミリ秒に変換する。
     * libwebrtc の C++ では capture_time_ms_ * 1e6 で渡されるため、
     * Unix ミリ秒ベースなら captureTimeNs / 1_000_000 で変換可能。
     * @param captureTimeNs ナノ秒単位のキャプチャ時刻（Unix epoch または NTP ベース）
     * @return Unix epoch からのミリ秒。不正な範囲の場合は null
     */
    fun captureTimeNsToUnixMs(captureTimeNs: Long): Long? {
        if (captureTimeNs <= 0) return null
        val unixMs = captureTimeNs / 1_000_000
        // 妥当性チェック: 2020年〜2035年程度
        if (unixMs < 1_578_000_000_000L || unixMs > 2_050_000_000_000L) return null
        return unixMs
    }
}
