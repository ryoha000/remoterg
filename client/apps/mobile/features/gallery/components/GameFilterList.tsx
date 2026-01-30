import { View, FlatList, TouchableOpacity, StyleSheet } from "react-native"
import { Text } from "@/components/ui/text"
import { useRecentGames } from "../hooks/useGameFilter"
import { cn } from "@/lib/utils"

interface GameFilterListProps {
  selectedTitle: string | null
  onSelectTitle: (title: string | null) => void
}

export function GameFilterList({ selectedTitle, onSelectTitle }: GameFilterListProps) {
  const { data: gameTitles } = useRecentGames()

  if (!gameTitles || gameTitles.length === 0) return null

  const uniqueTitles = Array.from(new Set(gameTitles)).filter(title => title && title.trim() !== "")

  if (uniqueTitles.length === 0) return null

  return (
    <View className="py-2 border-b border-zinc-900/50 bg-zinc-950/30">
        <FlatList
        horizontal
        data={["All", ...uniqueTitles] as string[]}
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={{ paddingHorizontal: 16, gap: 8 }}
        keyExtractor={(item) => item}
        renderItem={({ item }: { item: string }) => {
            const isSelected = item === "All" ? selectedTitle === null : selectedTitle === item
            return (
            <TouchableOpacity
                onPress={() => onSelectTitle(item === "All" ? null : item)}
                className={cn(
                "px-3 py-1.5 rounded-full border",
                isSelected
                    ? "bg-zinc-100 border-zinc-100"
                    : "bg-zinc-900/50 border-zinc-800"
                )}
            >
                <Text
                className={cn(
                    "text-xs font-medium",
                    isSelected ? "text-zinc-900" : "text-zinc-400"
                )}
                >
                {item}
                </Text>
            </TouchableOpacity>
            )
        }}
        />
    </View>
  )
}
