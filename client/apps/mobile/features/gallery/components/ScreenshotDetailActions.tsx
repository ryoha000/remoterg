import { View, Text, ViewStyle, StyleProp } from "react-native"
import { Ionicons } from "@expo/vector-icons"
import Animated, { AnimatedStyle } from "react-native-reanimated"

import { Button } from "@/components/ui/button"

interface ScreenshotDetailActionsProps {
  onTwitterShare: () => void
  onGenericShare: () => void
  onDelete: () => void
  style?: StyleProp<AnimatedStyle<ViewStyle>>
}

export const ScreenshotDetailActions = ({
  onTwitterShare,
  onGenericShare,
  onDelete,
  style,
}: ScreenshotDetailActionsProps) => {
  return (
    <Animated.View
      style={[
        {
          position: "absolute",
          bottom: 0,
          left: 0,
        },
        style,
      ]}
      pointerEvents="box-none"
    >
      <View className="bg-black/60 backdrop-blur-md pb-8 pt-4 flex-row justify-around items-center border-t border-white/10">
        <View className="items-center gap-1">
          <Button variant="ghost" size="icon" onPress={onTwitterShare}>
            <Ionicons name="logo-twitter" size={20} color="white" />
          </Button>
          <Text className="text-white text-xs">Twitter</Text>
        </View>
        <View className="items-center gap-1">
          <Button variant="ghost" size="icon" onPress={onGenericShare}>
            <Ionicons name="share-outline" size={20} color="white" />
          </Button>
          <Text className="text-white text-xs">共有</Text>
        </View>
        <View className="items-center gap-1">
          <Button variant="ghost" size="icon" onPress={onDelete}>
            <Ionicons name="trash-outline" size={20} color="white" />
          </Button>
          <Text className="text-white text-xs">ゴミ箱</Text>
        </View>
      </View>
    </Animated.View>
  )
}
