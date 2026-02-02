import { Ionicons } from "@expo/vector-icons"
import { useState, useCallback, useMemo, useRef, useEffect } from "react"
import {
  View,
  TextInput,
  TouchableOpacity,
  ScrollView,
  Animated,
  Pressable,
} from "react-native"

import { Text } from "@/components/ui/text"

export interface SearchFilters {
  text?: string
  since?: Date
  until?: Date
  gameTitle?: string
  isFavorite?: boolean
}

interface SearchToken {
  type: "text" | "since" | "until" | "game" | "favorite"
  value: string
  displayValue: string
}

interface SearchPanelProps {
  value: SearchFilters
  onChange: (filters: SearchFilters) => void
  gameTitles?: string[]
  placeholder?: string
}

// Parse search query into tokens and filters
export function parseSearchQuery(query: string): {
  tokens: SearchToken[]
  filters: SearchFilters
} {
  const tokens: SearchToken[] = []
  const filters: SearchFilters = {}

  if (!query.trim()) {
    return { tokens, filters }
  }

  // Parse date filters: since:YYYY-MM-DD or since:YYYY/MM/DD
  const sinceMatch = query.match(/since:(\d{4}[-/]\d{2}[-/]\d{2})/)
  if (sinceMatch) {
    const dateStr = sinceMatch[1].replace(/\//g, "-")
    filters.since = new Date(dateStr)
    tokens.push({
      type: "since",
      value: sinceMatch[0],
      displayValue: `since:${dateStr}`,
    })
  }

  // Parse date filters: until:YYYY-MM-DD or until:YYYY/MM/DD
  const untilMatch = query.match(/until:(\d{4}[-/]\d{2}[-/]\d{2})/)
  if (untilMatch) {
    const dateStr = untilMatch[1].replace(/\//g, "-")
    filters.until = new Date(dateStr)
    filters.until.setHours(23, 59, 59, 999) // End of day
    tokens.push({
      type: "until",
      value: untilMatch[0],
      displayValue: `until:${dateStr}`,
    })
  }

  // Parse favorite filter
  if (query.includes("is:favorite")) {
    filters.isFavorite = true
    tokens.push({
      type: "favorite",
      value: "is:favorite",
      displayValue: "お気に入り",
    })
  }

  // Extract remaining text (remove filter syntax)
  const remainingText = query
    .replace(/since:\d{4}[-/]\d{2}[-/]\d{2}/g, "")
    .replace(/until:\d{4}[-/]\d{2}[-/]\d{2}/g, "")
    .replace(/is:favorite/g, "")
    .trim()

  if (remainingText) {
    filters.text = remainingText.toLowerCase()
    tokens.push({
      type: "text",
      value: remainingText,
      displayValue: remainingText,
    })
  }

  return { tokens, filters }
}

// Format filters back to query string
export function formatSearchQuery(filters: SearchFilters): string {
  const parts: string[] = []

  if (filters.text) {
    parts.push(filters.text)
  }
  if (filters.since) {
    const dateStr = filters.since.toISOString().split("T")[0]
    parts.push(`since:${dateStr}`)
  }
  if (filters.until) {
    const dateStr = filters.until.toISOString().split("T")[0]
    parts.push(`until:${dateStr}`)
  }
  if (filters.isFavorite) {
    parts.push("is:favorite")
  }
  if (filters.gameTitle) {
    parts.push(`game:"${filters.gameTitle}"`)
  }

  return parts.join(" ")
}

// Helper to format date for display
function formatDate(date: Date): string {
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const target = new Date(date.getFullYear(), date.getMonth(), date.getDate())

  if (target.getTime() === today.getTime()) {
    return "今日"
  }

  const yesterday = new Date(today)
  yesterday.setDate(yesterday.getDate() - 1)
  if (target.getTime() === yesterday.getTime()) {
    return "昨日"
  }

  return `${date.getMonth() + 1}月${date.getDate()}日`
}

// Date suggestion item
interface DateSuggestion {
  label: string
  since?: Date
  until?: Date
  icon: keyof typeof Ionicons.glyphMap
}

const DATE_SUGGESTIONS: DateSuggestion[] = [
  {
    label: "今日",
    since: (() => {
      const d = new Date()
      d.setHours(0, 0, 0, 0)
      return d
    })(),
    until: (() => {
      const d = new Date()
      d.setHours(23, 59, 59, 999)
      return d
    })(),
    icon: "today",
  },
  {
    label: "昨日",
    since: (() => {
      const d = new Date(Date.now() - 86400000)
      d.setHours(0, 0, 0, 0)
      return d
    })(),
    until: (() => {
      const d = new Date(Date.now() - 86400000)
      d.setHours(23, 59, 59, 999)
      return d
    })(),
    icon: "calendar",
  },
  {
    label: "過去7日間",
    since: (() => {
      const d = new Date(Date.now() - 7 * 86400000)
      d.setHours(0, 0, 0, 0)
      return d
    })(),
    until: (() => {
      const d = new Date()
      d.setHours(23, 59, 59, 999)
      return d
    })(),
    icon: "time",
  },
  {
    label: "過去30日間",
    since: (() => {
      const d = new Date(Date.now() - 30 * 86400000)
      d.setHours(0, 0, 0, 0)
      return d
    })(),
    until: (() => {
      const d = new Date()
      d.setHours(23, 59, 59, 999)
      return d
    })(),
    icon: "calendar-number",
  },
]

// Search token chip component
function TokenChip({ token, onRemove }: { token: SearchToken; onRemove: () => void }) {
  const getChipColor = () => {
    switch (token.type) {
      case "since":
      case "until":
        return "bg-blue-500/20 border-blue-500/30"
      case "favorite":
        return "bg-pink-500/20 border-pink-500/30"
      case "game":
        return "bg-purple-500/20 border-purple-500/30"
      default:
        return "bg-zinc-700 border-zinc-600"
    }
  }

  const getTextColor = () => {
    switch (token.type) {
      case "since":
      case "until":
        return "text-blue-400"
      case "favorite":
        return "text-pink-400"
      case "game":
        return "text-purple-400"
      default:
        return "text-zinc-300"
    }
  }

  return (
    <View className={`flex-row items-center px-2 py-1 rounded-md border ${getChipColor()}`}>
      <Text className={`text-sm ${getTextColor()}`} numberOfLines={1}>
        {token.displayValue}
      </Text>
      <TouchableOpacity onPress={onRemove} className="ml-1 p-0.5">
        <Ionicons name="close-circle" size={14} color="#71717a" />
      </TouchableOpacity>
    </View>
  )
}

export function SearchPanel({
  value,
  onChange,
  gameTitles = [],
  placeholder = "検索...",
}: SearchPanelProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [inputValue, setInputValue] = useState("")
  const [showHelp, setShowHelp] = useState(false)
  const inputRef = useRef<TextInput>(null)
  const slideAnim = useRef(new Animated.Value(0)).current

  const { tokens } = useMemo(() => {
    const query = formatSearchQuery(value)
    const parsed = parseSearchQuery(query)
    return { tokens: parsed.tokens }
  }, [value])

  // Sync input value with external filters
  useEffect(() => {
    setInputValue(formatSearchQuery(value))
  }, [value])

  const handleInputChange = (text: string) => {
    setInputValue(text)
    const { filters } = parseSearchQuery(text)
    onChange(filters)
  }

  const handleRemoveToken = (tokenToRemove: SearchToken) => {
    let newQuery = inputValue

    // Remove the specific token from query
    if (tokenToRemove.type === "since" || tokenToRemove.type === "until") {
      newQuery = newQuery.replace(tokenToRemove.value, "")
    } else if (tokenToRemove.type === "favorite") {
      newQuery = newQuery.replace("is:favorite", "")
    } else if (tokenToRemove.type === "text") {
      newQuery = newQuery.replace(tokenToRemove.value, "")
    }

    newQuery = newQuery.replace(/\s+/g, " ").trim()
    setInputValue(newQuery)
    const { filters } = parseSearchQuery(newQuery)
    onChange(filters)
  }

  const applyDateSuggestion = (suggestion: DateSuggestion) => {
    const newFilters = { ...value }

    if (suggestion.since) {
      newFilters.since = suggestion.since
    }
    if (suggestion.until) {
      newFilters.until = suggestion.until
    }

    onChange(newFilters)
    setIsOpen(false)
  }

  const clearAllFilters = () => {
    setInputValue("")
    onChange({})
  }

  const hasActiveFilters =
    value.text || value.since || value.until || value.isFavorite || value.gameTitle

  // Animate panel open/close
  useEffect(() => {
    Animated.timing(slideAnim, {
      toValue: isOpen ? 1 : 0,
      duration: 200,
      useNativeDriver: true,
    }).start()
  }, [isOpen, slideAnim])

  const translateY = slideAnim.interpolate({
    inputRange: [0, 1],
    outputRange: [-10, 0],
  })

  const opacity = slideAnim.interpolate({
    inputRange: [0, 1],
    outputRange: [0, 1],
  })

  return (
    <View className="relative">
      <View className="flex-row items-center gap-2">
        <TouchableOpacity
          activeOpacity={1}
          onPress={() => {
            setIsOpen(true)
            setTimeout(() => inputRef.current?.focus(), 100)
          }}
          className="flex-row items-center gap-2 px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-lg"
          style={{ maxWidth: 200 }}
        >
          <Ionicons name="search" size={18} color="#71717a" />

          {tokens.length > 0 ? (
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              className="flex-1"
              contentContainerClassName="gap-1"
            >
              {tokens.map((token, index) => (
                <TokenChip
                  key={`${token.type}-${index}`}
                  token={token}
                  onRemove={() => handleRemoveToken(token)}
                />
              ))}
            </ScrollView>
          ) : (
            <Text className="flex-1 text-zinc-500 text-sm">{placeholder}</Text>
          )}

          {hasActiveFilters && (
            <TouchableOpacity onPress={clearAllFilters} className="p-1">
              <Ionicons name="close-circle" size={18} color="#71717a" />
            </TouchableOpacity>
          )}

          <TouchableOpacity onPress={() => setShowHelp(!showHelp)} className="p-1">
            <Ionicons name="help-circle-outline" size={18} color="#71717a" />
          </TouchableOpacity>
        </TouchableOpacity>
      </View>

      {/* Expanded Search Panel */}
      {isOpen && (
        <Animated.View
          className="absolute top-full left-0 mt-2 bg-zinc-900 border border-zinc-800 rounded-xl shadow-lg z-50"
          style={{ transform: [{ translateY }], opacity, width: 320 }}
        >
          {/* Input field for editing */}
          <View className="p-3 border-b border-zinc-800">
            <View className="flex-row items-center gap-2 bg-zinc-800/50 rounded-lg px-3 py-2">
              <Ionicons name="search" size={18} color="#71717a" />
              <TextInput
                ref={inputRef}
                value={inputValue}
                onChangeText={handleInputChange}
                placeholder={placeholder}
                placeholderTextColor="#71717a"
                className="flex-1 text-white text-sm"
                autoFocus
              />
              <TouchableOpacity onPress={() => setIsOpen(false)} className="p-1">
                <Ionicons name="chevron-up" size={18} color="#71717a" />
              </TouchableOpacity>
            </View>
          </View>

          {/* Quick Date Filters */}
          <View className="p-3 border-b border-zinc-800">
            <Text className="text-zinc-400 text-xs font-medium mb-2 uppercase tracking-wide">
              日付で絞り込み
            </Text>
            <View className="flex-row flex-wrap gap-2">
              {DATE_SUGGESTIONS.map((suggestion) => (
                <TouchableOpacity
                  key={suggestion.label}
                  onPress={() => applyDateSuggestion(suggestion)}
                  className="flex-row items-center gap-2 px-3 py-2 bg-zinc-800/50 rounded-lg active:bg-zinc-700"
                >
                  <Ionicons name={suggestion.icon} size={16} color="#a1a1aa" />
                  <Text className="text-zinc-300 text-sm">{suggestion.label}</Text>
                </TouchableOpacity>
              ))}
            </View>
          </View>

          {/* Quick Filters */}
          <View className="p-3 border-b border-zinc-800">
            <Text className="text-zinc-400 text-xs font-medium mb-2 uppercase tracking-wide">
              クイックフィルター
            </Text>
            <View className="flex-row flex-wrap gap-2">
              <TouchableOpacity
                onPress={() => {
                  const newValue = value.isFavorite
                    ? inputValue.replace("is:favorite", "").trim()
                    : `${inputValue} is:favorite`.trim()
                  setInputValue(newValue)
                  onChange({ ...value, isFavorite: !value.isFavorite })
                }}
                className={`flex-row items-center gap-2 px-3 py-2 rounded-lg ${
                  value.isFavorite
                    ? "bg-pink-500/20 border border-pink-500/30"
                    : "bg-zinc-800/50 active:bg-zinc-700"
                }`}
              >
                <Ionicons
                  name={value.isFavorite ? "heart" : "heart-outline"}
                  size={16}
                  color={value.isFavorite ? "#f91980" : "#a1a1aa"}
                />
                <Text className={`text-sm ${value.isFavorite ? "text-pink-400" : "text-zinc-300"}`}>
                  お気に入りのみ
                </Text>
              </TouchableOpacity>
            </View>
          </View>

          {/* Active Filters Summary */}
          {(value.since || value.until) && (
            <View className="p-3 bg-zinc-800/30">
              <Text className="text-zinc-400 text-xs mb-2">現在のフィルター</Text>
              <View className="flex-row flex-wrap gap-2">
                {value.since && (
                  <View className="flex-row items-center gap-1 px-2 py-1 bg-blue-500/20 rounded">
                    <Ionicons name="calendar" size={12} color="#60a5fa" />
                    <Text className="text-blue-400 text-xs">以降: {formatDate(value.since)}</Text>
                  </View>
                )}
                {value.until && (
                  <View className="flex-row items-center gap-1 px-2 py-1 bg-blue-500/20 rounded">
                    <Ionicons name="calendar" size={12} color="#60a5fa" />
                    <Text className="text-blue-400 text-xs">以前: {formatDate(value.until)}</Text>
                  </View>
                )}
              </View>
            </View>
          )}

          {/* Search Help */}
          {showHelp && (
            <View className="p-3 bg-zinc-950/50 border-t border-zinc-800">
              <Text className="text-zinc-400 text-xs font-medium mb-2">検索の使い方</Text>
              <View className="gap-1">
                <Text className="text-zinc-500 text-xs">
                  <Text className="text-blue-400">since:2024-01-01</Text> - 指定日以降の画像
                </Text>
                <Text className="text-zinc-500 text-xs">
                  <Text className="text-blue-400">until:2024-01-31</Text> - 指定日以前の画像
                </Text>
                <Text className="text-zinc-500 text-xs">
                  <Text className="text-pink-400">is:favorite</Text> - お気に入りのみ表示
                </Text>
                <Text className="text-zinc-500 text-xs">
                  キーワード - ファイル名やタイトルで検索
                </Text>
              </View>
            </View>
          )}
        </Animated.View>
      )}

      {/* Backdrop to close panel */}
      {isOpen && (
        <Pressable
          onPress={() => setIsOpen(false)}
          style={{
            position: "absolute",
            top: -500,
            left: -500,
            right: -500,
            bottom: -500,
            zIndex: -1,
          }}
        />
      )}
    </View>
  )
}
