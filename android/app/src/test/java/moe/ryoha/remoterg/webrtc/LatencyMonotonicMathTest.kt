package moe.ryoha.remoterg.webrtc

import org.junit.Assert.assertEquals
import org.junit.Test

class LatencyMonotonicMathTest {
    @Test
    fun median_and_smoothing_are_stable() {
        val median = LatencyMonotonicMath.median(listOf(10.0, 20.0, 30.0, 40.0, 50.0))
        assertEquals(30.0, median, 0.0001)

        val smoothed = LatencyMonotonicMath.smoothEstimate(old = 100.0, latest = 200.0, alpha = 0.1)
        assertEquals(110.0, smoothed, 0.0001)
    }

    @Test
    fun derive_sync_sample_calculates_expected_values() {
        val c1 = 1000.0
        val s2 = 1500.0
        val s3 = 1505.0
        val c4 = 1020.0

        val derived = LatencyMonotonicMath.deriveSyncSample(
            c1 = c1,
            s2 = s2,
            s3 = s3,
            c4 = c4
        )

        assertEquals(15.0, derived.rttMs, 0.0001)
        assertEquals(492.5, derived.offsetMonoMs, 0.0001)
    }

    @Test
    fun hostd_mono_to_client_mono_uses_offset() {
        val tCapHostdMonoMs = 1200.0
        val offsetMonoMs = 500.0

        val tCapClientMono = LatencyMonotonicMath.hostdMonoToClientMonoMs(
            tCapHostdMonoMs = tCapHostdMonoMs,
            offsetMonoMs = offsetMonoMs
        )

        assertEquals(700.0, tCapClientMono, 0.0001)
    }
}
