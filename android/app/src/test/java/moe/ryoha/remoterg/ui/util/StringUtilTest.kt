package moe.ryoha.remoterg.ui.util

import org.junit.Assert.assertEquals
import org.junit.Test

class StringUtilTest {

    @Test
    fun testLongestCommonPrefix() {
        val list1 = listOf("GameTitle ver.1.0", "GameTitle ver.1.1", "GameTitle ver.2.0")
        assertEquals("GameTitle ver.", StringUtil.longestCommonPrefix(list1))

        val list2 = listOf("SameName", "SameName", "SameName")
        assertEquals("SameName", StringUtil.longestCommonPrefix(list2))

        val list3 = listOf("A", "B", "C")
        assertEquals("", StringUtil.longestCommonPrefix(list3))

        val list4 = listOf("GameTitle Chapter 1", "GameTitle Chapter 2")
        assertEquals("GameTitle Chapter ", StringUtil.longestCommonPrefix(list4))

        val list5 = emptyList<String>()
        assertEquals("", StringUtil.longestCommonPrefix(list5))

        val list6 = listOf("SingleGame")
        assertEquals("SingleGame", StringUtil.longestCommonPrefix(list6))
    }

    @Test
    fun testCleanPrefix() {
        assertEquals("GameTitle", StringUtil.cleanPrefix("GameTitle - "))
        assertEquals("GameTitle", StringUtil.cleanPrefix("GameTitle : "))
        assertEquals("GameTitle", StringUtil.cleanPrefix("GameTitle | "))
        assertEquals("GameTitle", StringUtil.cleanPrefix("GameTitle > "))
        assertEquals("GameTitle", StringUtil.cleanPrefix("GameTitle   "))
        assertEquals("GameTitle ver.", StringUtil.cleanPrefix("GameTitle ver."))
        assertEquals("GameTitle Chapter", StringUtil.cleanPrefix("GameTitle Chapter "))
        assertEquals("Title", StringUtil.cleanPrefix("Title"))
    }
}
