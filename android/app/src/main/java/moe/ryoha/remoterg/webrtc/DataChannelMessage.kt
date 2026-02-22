package moe.ryoha.remoterg.webrtc

sealed class DataChannelMessage {
    data class Text(val text: String) : DataChannelMessage()
    data class Binary(val data: ByteArray) : DataChannelMessage()
    
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (javaClass != other?.javaClass) return false

        other as DataChannelMessage

        when (this) {
            is Text -> if (text != (other as Text).text) return false
            is Binary -> if (!data.contentEquals((other as Binary).data)) return false
        }
        return true
    }

    override fun hashCode(): Int {
        return when (this) {
            is Text -> text.hashCode()
            is Binary -> data.contentHashCode()
        }
    }
}
