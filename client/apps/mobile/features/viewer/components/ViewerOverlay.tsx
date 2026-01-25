import { View, TouchableOpacity, StyleSheet } from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { cn } from "@/lib/utils";
import Animated, { FadeIn, FadeOut } from "react-native-reanimated";
import { Button } from "@/components/ui/button";
import { Text } from "@/components/ui/text";

interface ViewerOverlayProps {
  visible: boolean;
  status: string;
  onDisconnect: () => void;
  onRotate?: () => void;
}

export function ViewerOverlay({ visible, status, onDisconnect, onRotate }: ViewerOverlayProps) {
  if (!visible) return null;

  return (
    <Animated.View 
      entering={FadeIn.duration(200)} 
      exiting={FadeOut.duration(200)}
      style={StyleSheet.absoluteFill} 
      pointerEvents="box-none"
    >
      {/* Top Bar */}
      <View className="absolute top-0 left-0 right-0 p-4 pt-12 flex-row justify-between items-center bg-black/50">
        <View className="flex-row items-center gap-2">
          <View className={cn("w-2 h-2 rounded-full", status.includes("connected") || status.includes("PC: connected") ? "bg-green-500" : "bg-yellow-500")} />
          <Text className="text-white font-medium">{status}</Text>
        </View>
      </View>

      {/* Bottom Bar */}
      <View className="absolute bottom-0 left-0 right-0 p-6 pb-10 flex-row justify-between items-center bg-black/50 gap-4">
        <Button 
          variant="outline" 
          size="icon" 
          className="rounded-full bg-white/20 border-0 backdrop-blur-md"
          onPress={onRotate}
        >
           <Ionicons name="sync" size={24} color="white" />
        </Button>

        <Button 
            variant="destructive" 
            size="icon"
            className="rounded-full backdrop-blur-md"
            onPress={onDisconnect}
        >
          <Ionicons name="close" size={24} color="white" />
        </Button>
      </View>
    </Animated.View>
  );
}
