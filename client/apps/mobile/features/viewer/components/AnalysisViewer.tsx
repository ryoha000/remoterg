import { Ionicons } from "@expo/vector-icons"
import { View } from "react-native"

import { Badge } from "@/components/ui/badge"
import { Text } from "@/components/ui/text"

import { AnalysisResult } from "../types"

interface AnalysisViewerProps {
  analysis: AnalysisResult
}

export function AnalysisViewer({ analysis }: AnalysisViewerProps) {
  return (
    <View className="gap-6">
      {/* Scene Info */}
      {analysis.scene_info && (
        <View className="gap-2">
          <Text className="text-sm font-semibold text-zinc-400 flex flex-row items-center gap-2">
            <Ionicons name="location-sharp" size={14} color="#a1a1aa" /> Scene
          </Text>
          <View className="bg-zinc-900/50 rounded-md p-3 border border-zinc-800 gap-1">
            <View className="flex-row">
              <Text className="text-zinc-500 w-20 text-sm">Location</Text>
              <Text className="text-zinc-200 text-sm flex-1">{analysis.scene_info.location}</Text>
            </View>
            <View className="flex-row">
              <Text className="text-zinc-500 w-20 text-sm">Time</Text>
              <Text className="text-zinc-200 text-sm flex-1">
                {analysis.scene_info.time_of_day}
              </Text>
            </View>
            <View className="flex-row">
              <Text className="text-zinc-500 w-20 text-sm">Mood</Text>
              <Text className="text-zinc-200 text-sm flex-1">{analysis.scene_info.atmosphere}</Text>
            </View>
          </View>
        </View>
      )}

      {/* Dialogue */}
      {analysis.dialogue && (
        <View className="gap-2">
          <Text className="text-sm font-semibold text-zinc-400 flex flex-row items-center gap-2">
            <Ionicons name="chatbubble-ellipses-outline" size={14} color="#a1a1aa" /> Dialogue
          </Text>
          <View className="bg-zinc-900/50 rounded-md p-3 border border-zinc-800">
            <Text className="font-semibold text-indigo-300 mb-1">{analysis.dialogue.speaker}</Text>
            <Text className="text-zinc-200 leading-relaxed">{analysis.dialogue.text}</Text>
          </View>
        </View>
      )}

      {/* Characters */}
      {analysis.characters && analysis.characters.length > 0 && (
        <View className="gap-2">
          <Text className="text-sm font-semibold text-zinc-400 flex flex-row items-center gap-2">
            <Ionicons name="people-outline" size={14} color="#a1a1aa" /> Characters
          </Text>
          <View className="gap-3">
            {analysis.characters.map((char, i) => (
              <View key={i} className="bg-zinc-900/50 rounded-md p-3 border border-zinc-800">
                <View className="flex-row items-center justify-between mb-2">
                  <Text className="font-semibold text-emerald-300">{char.name}</Text>
                  <Text className="text-xs text-zinc-500 uppercase">{char.position}</Text>
                </View>
                <Text className="text-zinc-300 mb-2 text-sm">{char.visual_description}</Text>
                <View className="flex-row flex-wrap gap-1">
                  {char.expression_tags?.map((tag, j) => (
                    <Badge key={j} variant="secondary" className="bg-zinc-800">
                      <Text className="text-zinc-400 text-[10px]">{tag}</Text>
                    </Badge>
                  ))}
                </View>
              </View>
            ))}
          </View>
        </View>
      )}
    </View>
  )
}
