package moe.ryoha.remoterg.webrtc

object LatencyMonotonicMath {
    data class SyncDerived(
        val rttMs: Double,
        val offsetMonoMs: Double
    )

    fun deriveSyncSample(
        c1: Double,
        s2: Double,
        s3: Double,
        c4: Double
    ): SyncDerived {
        val rtt = (c4 - c1) - (s3 - s2)
        val offsetMono = ((s2 - c1) + (s3 - c4)) / 2.0
        return SyncDerived(
            rttMs = rtt,
            offsetMonoMs = offsetMono
        )
    }

    fun median(values: List<Double>): Double {
        if (values.isEmpty()) return 0.0
        val sorted = values.sorted()
        return sorted[sorted.size / 2]
    }

    fun smoothEstimate(old: Double?, latest: Double, alpha: Double = 0.1): Double {
        if (old == null) return latest
        return alpha * latest + (1 - alpha) * old
    }

    fun hostdMonoToClientMonoMs(
        tCapHostdMonoMs: Double,
        offsetMonoMs: Double
    ): Double {
        return tCapHostdMonoMs - offsetMonoMs
    }
}
