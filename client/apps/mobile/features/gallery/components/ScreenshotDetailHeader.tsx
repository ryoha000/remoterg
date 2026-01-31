import { Ionicons } from "@expo/vector-icons"
import { ViewStyle, StyleProp } from "react-native"
import { View } from "react-native"
import Animated, { AnimatedStyle } from "react-native-reanimated"
import { useSafeAreaInsets } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"

interface ScreenshotDetailHeaderProps {
  onClose: () => void
  isFavorite?: boolean
  onToggleFavorite?: () => void
  style?: StyleProp<AnimatedStyle<ViewStyle>>
}

export const ScreenshotDetailHeader = ({
  onClose,
  isFavorite = false,
  onToggleFavorite,
  style,
}: ScreenshotDetailHeaderProps) => {
  const insets = useSafeAreaInsets()

  return (
    <Animated.View
      style={[
        {
          position: "absolute",
          top: 0,
          left: 0,
          zIndex: 30,
          paddingTop: insets.top,
          backgroundColor: "rgba(0,0,0,0.4)",
        },
        style,
      ]}
    >
      <View className="flex-row items-center gap-2 px-4 py-2">
        <Button
          variant="ghost"
          size="icon"
          className="rounded-full bg-black/20 active:bg-white/20"
          onPress={onClose}
        >
          <Ionicons name="arrow-back" size={24} color="#ffffff" />
        </Button>
        <View className="flex-1" />
        <Button
          variant="ghost"
          size="icon"
          className="rounded-full active:bg-white/20"
          onPress={onToggleFavorite}
        >
          <Ionicons
            name={isFavorite ? "heart" : "heart-outline"}
            size={24}
            color={isFavorite ? "#f91980" : "#ffffff"}
          />
        </Button>
      </View>
    </Animated.View>
  )
}
