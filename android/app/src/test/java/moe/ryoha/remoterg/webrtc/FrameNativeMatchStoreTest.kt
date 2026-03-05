package moe.ryoha.remoterg.webrtc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class FrameNativeMatchStoreTest {
    @Test
    fun matches_when_frame_arrives_first() {
        val store = FrameNativeMatchStore(ttlMs = 1_000)
        val captureUnixMs = 1_700_000_000_100L

        assertNull(
            store.offerFrame(
                captureUnixMs = captureUnixMs,
                tCapHostdMonoMs = 1234.0,
                receivedElapsedMs = 10_000
            )
        )

        val matched = store.offerNative(
            captureUnixMs = captureUnixMs,
            timestampUs = 555_000L,
            receivedElapsedMs = 10_020
        )
        assertNotNull(matched)
        assertEquals(555_000L, matched!!.timestampUs)
        assertEquals(1234.0, matched.tCapHostdMonoMs, 0.0001)
    }

    @Test
    fun matches_when_native_arrives_first() {
        val store = FrameNativeMatchStore(ttlMs = 1_000)
        val captureUnixMs = 1_700_000_000_200L

        assertNull(
            store.offerNative(
                captureUnixMs = captureUnixMs,
                timestampUs = 666_000L,
                receivedElapsedMs = 20_000
            )
        )

        val matched = store.offerFrame(
            captureUnixMs = captureUnixMs,
            tCapHostdMonoMs = 2222.0,
            receivedElapsedMs = 20_010
        )
        assertNotNull(matched)
        assertEquals(666_000L, matched!!.timestampUs)
        assertEquals(2222.0, matched.tCapHostdMonoMs, 0.0001)
    }

    @Test
    fun evicts_expired_pending_entries_by_ttl() {
        val store = FrameNativeMatchStore(ttlMs = 100)
        val captureUnixMs = 1_700_000_000_300L

        assertNull(
            store.offerFrame(
                captureUnixMs = captureUnixMs,
                tCapHostdMonoMs = 3333.0,
                receivedElapsedMs = 1_000
            )
        )
        assertEquals(1, store.pendingFrameCount())

        val unmatched = store.offerNative(
            captureUnixMs = 1_700_000_000_301L,
            timestampUs = 777_000L,
            receivedElapsedMs = 1_250
        )
        assertNull(unmatched)
        assertEquals(0, store.pendingFrameCount())
        assertEquals(1, store.pendingNativeCount())
    }
}
