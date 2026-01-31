import { Ionicons } from "@expo/vector-icons"
import { View, Text, ViewStyle, StyleProp } from "react-native"
import Animated, { AnimatedStyle } from "react-native-reanimated"
import Toast from "react-native-toast-message"

import { Button } from "@/components/ui/button"

interface ScreenshotDetailActionsProps {
  onTwitterShare: () => Promise<void>
  onGenericShare: () => void
  onDelete: () => Promise<void>
  style?: StyleProp<AnimatedStyle<ViewStyle>>
}

export const ScreenshotDetailActions = ({
  onTwitterShare,
  onGenericShare,
  onDelete,
  style,
}: ScreenshotDetailActionsProps) => {
  const handleTwitterShare = async () => {
    try {
      await onTwitterShare()
    } catch (e: any) {
      Toast.show({
        type: "error",
        text1: "Twitterアプリが見つかりません",
        text2: "Twitterがインストールされているか確認してください。",
      })
    }
  }

  const handleDelete = async () => {
    try {
      await onDelete()
    } catch (e: any) {
      const message = e?.message || ""
      // Android scoped storage deletion permission denial often looks like:
      // "Call to function 'ExpoMediaLibrary.deleteAssetsAsync' has been rejected. -> Caused by: User didn't grant write permission to requested files."
      if (message.includes("User didn't grant") || message.includes("permission")) {
        Toast.show({
          type: "error",
          text1: "削除がキャンセルされました",
          text2: "削除するには、表示される確認ダイアログで「許可」を選択してください。",
        })
        return
      }

      Toast.show({
        type: "error",
        text1: "削除エラー",
        text2: "削除に失敗しました。もう一度お試しください。",
      })
    }
  }

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
          <Button
            variant="ghost"
            size="icon"
            onPress={handleTwitterShare}
            className="active:bg-white/20"
          >
            <Ionicons name="logo-twitter" size={20} color="white" />
          </Button>
          <Text className="text-white text-xs">Twitter</Text>
        </View>
        <View className="items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            onPress={onGenericShare}
            className="active:bg-white/20"
          >
            <Ionicons name="share-outline" size={20} color="white" />
          </Button>
          <Text className="text-white text-xs">共有</Text>
        </View>
        <View className="items-center gap-1">
          <Button variant="ghost" size="icon" onPress={handleDelete} className="active:bg-white/20">
            <Ionicons name="trash-outline" size={20} color="white" />
          </Button>
          <Text className="text-white text-xs">ゴミ箱</Text>
        </View>
      </View>
    </Animated.View>
  )
}
