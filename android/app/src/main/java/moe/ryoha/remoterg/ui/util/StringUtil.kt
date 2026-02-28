package moe.ryoha.remoterg.ui.util

object StringUtil {
    /**
     * Finds the longest common prefix among an array of strings.
     * Ignore case if needed, but for window titles exact case is usually fine.
     */
    fun longestCommonPrefix(strs: List<String>): String {
        if (strs.isEmpty()) return ""
        if (strs.size == 1) return strs[0]

        var prefix = strs[0]
        for (i in 1 until strs.size) {
            while (strs[i].indexOf(prefix) != 0) {
                prefix = prefix.substring(0, prefix.length - 1)
                if (prefix.isEmpty()) return ""
            }
        }
        return prefix
    }

    /**
     * Cleans up the extracted prefix by removing trailing spaces, hyphens, colons, etc.
     * Useful because prefixes like "GameTitle - " or "GameTitle: " leave hanging symbols.
     */
    fun cleanPrefix(prefix: String): String {
        return prefix.trimEnd { it.isWhitespace() || it == '-' || it == ':' || it == '|' || it == '>' }
    }
}
