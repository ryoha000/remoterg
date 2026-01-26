import { Ionicons } from "@expo/vector-icons"
import * as MediaLibrary from "expo-media-library"
import { useState, useEffect, useCallback } from "react"
import {
  Modal,
  View,
  FlatList,
  Image,
  TouchableOpacity,
  Dimensions,
  ActivityIndicator,
  Alert,
} from "react-native"
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { Text } from "@/components/ui/text"
import { cn } from "@/lib/utils"

interface GalleryModalProps {
  visible: boolean
  onClose: () => void
}

export function GalleryModal({ visible, onClose }: GalleryModalProps) {
  const [permissionResponse, requestPermission] = MediaLibrary.usePermissions()
  const [assets, setAssets] = useState<MediaLibrary.Asset[]>([])
  const [selectedAsset, setSelectedAsset] = useState<MediaLibrary.Asset | null>(null)
  const [loading, setLoading] = useState(false)
  const [hasNoAlbum, setHasNoAlbum] = useState(false)
  const insets = useSafeAreaInsets()

  const ALBUM_NAME = "RemoteRG" // このアプリで保存した画像のアルバム名

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

  // アセットの詳細情報を取得（ファイルサイズなど）
  const [assetInfo, setAssetInfo] = useState<MediaLibrary.AssetInfo | null>(null)
  useEffect(() => {
    const fetchInfo = async () => {
      if (selectedAsset) {
        const info = await MediaLibrary.getAssetInfoAsync(selectedAsset)
        setAssetInfo(info)
      } else {
        setAssetInfo(null)
      }
    }
    fetchInfo()
  }, [selectedAsset])

  const formatSize = (bytes?: number) => {
    if (!bytes) return "Unknown"
    const k = 1024
    const sizes = ["B", "KB", "MB", "GB"]
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i]
  }

  const renderGridItem = ({ item }: { item: MediaLibrary.Asset }) => {
    const width = Dimensions.get("window").width / 3 - 2
    return (
      <TouchableOpacity
        onPress={() => setSelectedAsset(item)}
        className="m-[1px] relative"
        style={{ width, height: width * 0.5625 }} // 16:9 aspect ratio roughly
      >
        <Image
          source={{ uri: item.uri }}
          style={{ width: "100%", height: "100%" }}
          resizeMode="cover"
        />
        {/* Web版のデザインに合わせたグラデーションオーバーレイとタイムスタンプ */}
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

  if (!permissionResponse) {
    return <View />
  }

  if (permissionResponse.status !== "granted" && visible) {
    return (
      <Modal visible={visible} animationType="slide" transparent={false}>
        <View className="flex-1 items-center justify-center bg-zinc-950 p-4">
          <Text className="text-white mb-4 text-center">
            アルバムへのアクセス権限が必要です。
          </Text>
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
    <Modal visible={visible} animationType="slide" transparent={false} onRequestClose={onClose}>
      <View className="flex-1 bg-zinc-950">
        <SafeAreaView className="flex-1" edges={["top", "left", "right"]}>
          {/* Header */}
          <View className="flex-row items-center justify-between px-4 py-2 border-b border-zinc-900 bg-zinc-950/50">
            <View className="flex-row items-center gap-2">
              {selectedAsset && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="rounded-full"
                  onPress={() => setSelectedAsset(null)}
                >
                  <Ionicons name="arrow-back" size={24} color="#a1a1aa" />
                </Button>
              )}
              <Text className="text-white text-lg font-medium">
                {selectedAsset ? "Screenshot Details" : "Screenshot Gallery"}
              </Text>
            </View>
            <Button variant="ghost" size="icon" className="rounded-full" onPress={onClose}>
              <Ionicons name="close" size={24} color="#a1a1aa" />
            </Button>
          </View>

          {/* Content */}
          <View className="flex-1 bg-zinc-900/30">
            {selectedAsset ? (
              // Detail View
              <View className="flex-1 flex-col">
                <View className="flex-1 items-center justify-center p-4">
                  <Image
                    source={{ uri: selectedAsset.uri }}
                    className="w-full h-full"
                    resizeMode="contain"
                  />
                </View>

                {/* Info Panel */}
                <View className="bg-zinc-900/50 border-t border-zinc-800 p-6 pb-8">
                  <Text className="text-lg font-semibold text-white mb-4">Metadata</Text>
                  <View className="gap-4">
                    <View>
                      <View className="flex-row items-center gap-2 mb-1">
                        <Ionicons name="calendar-outline" size={14} color="#71717a" />
                        <Text className="text-zinc-500 text-xs uppercase tracking-wider font-medium">
                          Timestamp
                        </Text>
                      </View>
                      <Text className="text-zinc-300 font-mono text-sm">
                        {new Date(selectedAsset.creationTime).toLocaleString()}
                      </Text>
                    </View>

                    <View>
                      <View className="flex-row items-center gap-2 mb-1">
                        <Ionicons name="resize-outline" size={14} color="#71717a" />
                        <Text className="text-zinc-500 text-xs uppercase tracking-wider font-medium">
                          Dimensions
                        </Text>
                      </View>
                      <Text className="text-zinc-300 font-mono text-sm">
                        {selectedAsset.width} x {selectedAsset.height}
                      </Text>
                    </View>

                    <View>
                      <View className="flex-row items-center gap-2 mb-1">
                        <Ionicons name="hardware-chip-outline" size={14} color="#71717a" />
                        <Text className="text-zinc-500 text-xs uppercase tracking-wider font-medium">
                          Size
                        </Text>
                      </View>
                      <Text className="text-zinc-300 font-mono text-sm">
                        {assetInfo ? formatSize(assetInfo.localUri ? undefined : 0) : "Loading..." /* React Native Asset doesn't always have size easy access without file info */}
                        {/* Note call MediaLibrary.getAssetInfoAsync might not return size on all platforms directly in bytes easily without FileSystem */}
                      </Text>
                    </View>
                  </View>
                </View>
              </View>
            ) : (
              // Grid View
              <View className="flex-1">
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
                    <Text className="text-zinc-500 text-center">
                      画像がありません。
                    </Text>
                  </View>
                ) : (
                  <FlatList
                    data={assets}
                    renderItem={renderGridItem}
                    keyExtractor={(item) => item.id}
                    numColumns={3}
                    contentContainerStyle={{ padding: 1 }}
                  />
                )}
              </View>
            )}
          </View>
        </SafeAreaView>
      </View>
    </Modal>
  )
}
