import { Ionicons } from "@expo/vector-icons"
import * as MediaLibrary from "expo-media-library"
import { useState, useEffect, useCallback, useRef, useMemo } from "react"
import {
  Modal,
  View,
  FlatList,
  Image,
  TouchableOpacity,
  Dimensions,
  ActivityIndicator,
  Animated,
  Easing,
} from "react-native"
import { GestureHandlerRootView } from "react-native-gesture-handler"
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Text } from "@/components/ui/text"

import { AssetOrigin, ScreenshotDetail, ScreenshotDetailRef } from "./ScreenshotDetail"

interface GalleryModalProps {
  visible: boolean
  onClose: () => void
}

interface JustifiedRow {
  id: string
  items: {
    asset: MediaLibrary.Asset
    width: number
    height: number
  }[]
  height: number
}

const SPACING = 2
const TARGET_ROW_HEIGHT = 240

const ThumbnailItem = ({
  item,
  width,
  height,
  onPress,
}: {
  item: MediaLibrary.Asset
  width: number
  height: number
  onPress: (asset: MediaLibrary.Asset, origin: AssetOrigin) => void
}) => {
  const itemRef = useRef<View>(null)

  const handlePress = () => {
    itemRef.current?.measureInWindow((x, y, w, h) => {
      onPress(item, { x, y, width: w, height: h })
    })
  }

  return (
    <TouchableOpacity
      ref={itemRef}
      onPress={handlePress}
      style={{ width, height, marginHorizontal: SPACING / 2 }}
    >
      <Image
        source={{ uri: item.uri }}
        style={{ width: "100%", height: "100%" }}
        resizeMode="contain"
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

function useJustifiedLayout(assets: MediaLibrary.Asset[], containerWidth: number) {
  return useMemo(() => {
    if (containerWidth === 0) return []

    const rows: JustifiedRow[] = []
    let currentRowItems: MediaLibrary.Asset[] = []
    let currentRowAspectRatio = 0

    // Available width removes spacing
    // We will account for spacing during distribution

    for (const asset of assets) {
      currentRowItems.push(asset)
      currentRowAspectRatio += asset.width / asset.height

      const estimatedWidth = currentRowAspectRatio * TARGET_ROW_HEIGHT

      // Decide if we should break the row
      // Basic logic: if adding this item makes it wider than container,
      // OR check if we are closer to container width by including it or excluding it.
      // For simplicity here, if it exceeds container width comfortably, we flush.

      // Actually Flickr/Google algo style:
      // Accumulate until height needed to match container width is < limit (e.g. not too tall)
      const heightToMatchWidth =
        (containerWidth - currentRowItems.length * SPACING) / currentRowAspectRatio

      // If the height is reasonable (not huge), we accept this row.
      // But usually we accumulate until we cover width, then shrink.

      if (estimatedWidth >= containerWidth) {
        // Finalize this row
        // Recalculate exact height to fill width
        const rowHeight =
          (containerWidth - currentRowItems.length * SPACING) / currentRowAspectRatio

        rows.push({
          id: currentRowItems[0].id,
          items: currentRowItems.map((a) => ({
            asset: a,
            width: (a.width / a.height) * rowHeight,
            height: rowHeight,
          })),
          height: rowHeight,
        })

        currentRowItems = []
        currentRowAspectRatio = 0
      }
    }

    // Handle last row (align left, don't expand)
    if (currentRowItems.length > 0) {
      rows.push({
        id: currentRowItems[0].id + "-last",
        items: currentRowItems.map((a) => ({
          asset: a,
          width: (a.width / a.height) * TARGET_ROW_HEIGHT,
          height: TARGET_ROW_HEIGHT,
        })),
        height: TARGET_ROW_HEIGHT,
      })
    }

    return rows
  }, [assets, containerWidth])
}

export function GalleryModal({ visible, onClose }: GalleryModalProps) {
  const [permissionResponse, requestPermission] = MediaLibrary.usePermissions()
  const [assets, setAssets] = useState<MediaLibrary.Asset[]>([])
  const [selectedAsset, setSelectedAsset] = useState<MediaLibrary.Asset | null>(null)
  const [selectedAssetOrigin, setSelectedAssetOrigin] = useState<AssetOrigin | null>(null)
  const [loading, setLoading] = useState(false)
  const [hasNoAlbum, setHasNoAlbum] = useState(false)
  const slideAnim = useRef(new Animated.Value(Dimensions.get("window").width)).current
  const screenshotDetailRef = useRef<ScreenshotDetailRef>(null) // Ref for detail view
  // Search state
  const [searchQuery, setSearchQuery] = useState("")
  // Refs
  const flatListRef = useRef<FlatList>(null)
  const itemRefs = useRef<Map<string, View>>(new Map()).current

  const ALBUM_NAME = "RemoteRG" // このアプリで保存した画像のアルバム名
  const screenWidth = Dimensions.get("window").width

  const justifiedRows = useJustifiedLayout(assets, screenWidth)

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

  // Provide measurement for closing animation
  const getAssetOrigin = useCallback(async (id: string): Promise<AssetOrigin | null> => {
    const view = itemRefs.get(id)
    if (!view) return null

    return new Promise((resolve) => {
      view.measureInWindow((x, y, width, height) => {
        resolve({ x, y, width, height })
      })
    })
  }, [])

  // Scroll to asset in grid (called when swiping between images in detail view)
  const scrollToAsset = useCallback(
    (index: number) => {
      // Find which row this index belongs to
      let count = 0
      let rowIndex = 0

      for (let i = 0; i < justifiedRows.length; i++) {
        const row = justifiedRows[i]
        // Check if the asset is in this row
        const rowHasItem = row.items.some((item) => item.asset.id === assets[index].id)
        if (rowHasItem) {
          rowIndex = i
          break
        }
      }

      flatListRef.current?.scrollToIndex({
        index: rowIndex,
        animated: false, // Instant scroll so it's ready for animation
        viewPosition: 0.5,
      })
    },
    [justifiedRows, assets],
  )

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
      statusBarTranslucent
    >
      <GestureHandlerRootView style={{ flex: 1 }}>
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
                <Button variant="ghost" size="icon" className="rounded-full" onPress={handleBack}>
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
                    ref={flatListRef}
                    data={justifiedRows}
                    renderItem={({ item: row }) => (
                      <View style={{ flexDirection: "row", marginBottom: SPACING }}>
                        {row.items.map((img: JustifiedRow["items"][number]) => (
                          <View
                            key={img.asset.id}
                            ref={(view) => {
                              if (view) itemRefs.set(img.asset.id, view)
                              else itemRefs.delete(img.asset.id)
                            }}
                          >
                            <ThumbnailItem
                              item={img.asset}
                              width={img.width}
                              height={img.height}
                              onPress={handleAssetSelect}
                            />
                          </View>
                        ))}
                      </View>
                    )}
                    keyExtractor={(item) => item.id}
                    contentContainerStyle={{ padding: SPACING / 2 }}
                    // Improve performance
                    initialNumToRender={5}
                    maxToRenderPerBatch={5}
                    windowSize={5}
                  />
                )}
              </View>
            </SafeAreaView>
            {selectedAsset && (
              <ScreenshotDetail
                ref={screenshotDetailRef}
                assets={assets}
                initialIndex={assets.findIndex((a) => a.id === selectedAsset.id)}
                origin={selectedAssetOrigin}
                getAssetOrigin={getAssetOrigin}
                onCurrentIndexChange={scrollToAsset}
                onClose={() => setSelectedAsset(null)}
              />
            )}
          </Animated.View>
        </View>
      </GestureHandlerRootView>
    </Modal>
  )
}
