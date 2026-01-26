import { Ionicons } from "@expo/vector-icons"
import * as MediaLibrary from "expo-media-library"
import { useState, useEffect, useCallback, useRef } from "react"
import {
  Modal,
  View,
  FlatList,
  Image,
  TouchableOpacity,
  Dimensions,
  ActivityIndicator,
  Alert,
  Animated,
  Easing,
} from "react-native"
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Text } from "@/components/ui/text"
import { cn } from "@/lib/utils"
import { AssetOrigin, ScreenshotDetail, ScreenshotDetailRef } from "./ScreenshotDetail"

interface GalleryModalProps {
  visible: boolean
  onClose: () => void
}

const ThumbnailItem = ({
  item,
  onPress,
}: {
  item: MediaLibrary.Asset
  onPress: (asset: MediaLibrary.Asset, origin: AssetOrigin) => void
}) => {
  const itemRef = useRef<View>(null)
  const width = Dimensions.get("window").width / 3 - 2

  const handlePress = () => {
    itemRef.current?.measureInWindow((x, y, w, h) => {
      onPress(item, { x, y, width: w, height: h })
    })
  }

  return (
    <TouchableOpacity
      ref={itemRef}
      onPress={handlePress}
      className="m-[1px] relative"
      style={{ width, height: width * 0.5625 }}
    >
      <Image
        source={{ uri: item.uri }}
        style={{ width: "100%", height: "100%" }}
        resizeMode="cover"
      />
      <View className="absolute bottom-0 left-0 right-0 p-1 bg-black/40">
        <Text className="text-white/90 text-[10px] font-mono">
          {new Date(item.creationTime).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
          })}
        </Text>
      </View>
    </TouchableOpacity>
  )
}

export function GalleryModal({ visible, onClose }: GalleryModalProps) {
  const [permissionResponse, requestPermission] = MediaLibrary.usePermissions()
  const [assets, setAssets] = useState<MediaLibrary.Asset[]>([])
  const [selectedAsset, setSelectedAsset] = useState<MediaLibrary.Asset | null>(null)
  const [selectedAssetOrigin, setSelectedAssetOrigin] = useState<AssetOrigin | null>(null)
  const [loading, setLoading] = useState(false)
  const [hasNoAlbum, setHasNoAlbum] = useState(false)
  const insets = useSafeAreaInsets()
  const slideAnim = useRef(new Animated.Value(Dimensions.get("window").width)).current
  const screenshotDetailRef = useRef<ScreenshotDetailRef>(null) // Ref for detail view
  // Search state
  const [searchQuery, setSearchQuery] = useState("")

  const ALBUM_NAME = "RemoteRG" // このアプリで保存した画像のアルバム名

  useEffect(() => {
    if (visible) {
      // Reset position just in case
      slideAnim.setValue(Dimensions.get("window").width)
      Animated.timing(slideAnim, {
        toValue: 0,
        duration: 300,
        useNativeDriver: true,
        easing: Easing.out(Easing.poly(4)),
      }).start()
    }
  }, [visible])

  const handleClose = useCallback(() => {
    Animated.timing(slideAnim, {
      toValue: Dimensions.get("window").width,
      duration: 250,
      useNativeDriver: true,
      easing: Easing.in(Easing.poly(4)),
    }).start(() => {
      onClose()
      // Reset state after close
      setSelectedAsset(null)
      setSelectedAssetOrigin(null)
      setSearchQuery("")
    })
  }, [onClose, slideAnim])

  const handleBack = () => {
    if (selectedAsset) {
      // Trigger detail view close animation if available
      if (screenshotDetailRef.current) {
        screenshotDetailRef.current.close()
      } else {
        setSelectedAsset(null)
      }
    } else {
      handleClose()
    }
  }

  const loadAssets = useCallback(async () => {
    if (permissionResponse?.status !== "granted") {
      return
    }

    setLoading(true)
    setHasNoAlbum(false)
    try {
      const album = await MediaLibrary.getAlbumAsync(ALBUM_NAME)
      if (!album) {
        setHasNoAlbum(true)
        setAssets([])
      } else {
        const result = await MediaLibrary.getAssetsAsync({
          album: album,
          mediaType: "photo",
          sortBy: ["creationTime"],
          first: 100, // 最新100件
        })
        setAssets(result.assets)
      }
    } catch (e) {
      console.error("Failed to load assets", e)
    } finally {
      setLoading(false)
    }
  }, [permissionResponse])

  useEffect(() => {
    if (visible && permissionResponse?.status === "granted") {
      loadAssets()
    }
  }, [visible, permissionResponse, loadAssets])

  useEffect(() => {
    if (visible && !permissionResponse) {
      requestPermission()
    }
  }, [visible, permissionResponse, requestPermission])



  const handleAssetSelect = useCallback((asset: MediaLibrary.Asset, origin: AssetOrigin) => {
    setSelectedAssetOrigin(origin)
    setSelectedAsset(asset)
  }, [])

  if (!permissionResponse) {
    return <View />
  }

  if (permissionResponse.status !== "granted" && visible) {
    return (
      <Modal visible={visible} animationType="slide" transparent={false}>
        <View className="flex-1 items-center justify-center bg-zinc-950 p-4">
          <Text className="text-white mb-4 text-center">アルバムへのアクセス権限が必要です。</Text>
          <Button onPress={requestPermission}>
            <Text>権限を許可する</Text>
          </Button>
          <Button variant="ghost" className="mt-4" onPress={onClose}>
            <Text className="text-zinc-400">閉じる</Text>
          </Button>
        </View>
      </Modal>
    )
  }

  return (
    <Modal
      visible={visible}
      animationType="none"
      transparent={true}
      onRequestClose={handleBack}
    >
      <View className="flex-1 bg-black/50">
        <Animated.View
          style={{
            transform: [{ translateX: slideAnim }],
            flex: 1,
          }}
          className="bg-zinc-950"
        >
          <SafeAreaView className="flex-1" edges={["top", "left", "right"]}>
            {/* Header */}
            <View className="flex-row items-center gap-2 px-4 py-2 border-b border-zinc-900 bg-zinc-950/50">
              <Button
                variant="ghost"
                size="icon"
                className="rounded-full"
                onPress={handleBack}
              >
                <Ionicons name="arrow-back" size={24} color="#a1a1aa" />
              </Button>
              
              <View className="flex-1">
                <Input
                  placeholder="Search..."
                  value={searchQuery}
                  onChangeText={setSearchQuery}
                  className="h-10 bg-zinc-900 border-zinc-800 text-white placeholder:text-zinc-500"
                />
              </View>
            </View>

            {/* Grid View */}
            <View className="flex-1 bg-zinc-900/30">

              {loading ? (
                <View className="flex-1 items-center justify-center">
                  <ActivityIndicator size="large" color="#a855f7" />
                </View>
              ) : hasNoAlbum ? (
                <View className="flex-1 items-center justify-center p-8 gap-4">
                  <View className="w-16 h-16 rounded-full bg-zinc-900 flex items-center justify-center">
                    <Ionicons name="images-outline" size={32} color="#3f3f46" />
                  </View>
                  <Text className="text-zinc-500 text-center">
                    "{ALBUM_NAME}" アルバムが見つかりません。
                    {"\n"}スクリーンショットを撮影するとここに表示されます。
                  </Text>
                </View>
              ) : assets.length === 0 ? (
                <View className="flex-1 items-center justify-center p-8 gap-4">
                  <View className="w-16 h-16 rounded-full bg-zinc-900 flex items-center justify-center">
                    <Ionicons name="images-outline" size={32} color="#3f3f46" />
                  </View>
                  <Text className="text-zinc-500 text-center">画像がありません。</Text>
                </View>
              ) : (
                <FlatList
                  data={assets}
                  renderItem={({ item }) => (
                    <ThumbnailItem item={item} onPress={handleAssetSelect} />
                  )}
                  keyExtractor={(item) => item.id}
                  numColumns={3}
                  contentContainerStyle={{ padding: 1 }}
                />
              )}
            </View>

        </SafeAreaView>
        {selectedAsset && (
          <ScreenshotDetail
            ref={screenshotDetailRef}
            asset={selectedAsset}
            origin={selectedAssetOrigin}
            onClose={() => setSelectedAsset(null)}
          />
        )}
      </Animated.View>
      </View>
    </Modal>
  )
}
