import { Ionicons } from "@expo/vector-icons"
import { Image } from "expo-image"
import * as MediaLibrary from "expo-media-library"
import { StatusBar } from "expo-status-bar"
import { useState, useEffect, useCallback, useRef, useMemo } from "react"
import { View, FlatList, TouchableOpacity, Dimensions, ActivityIndicator } from "react-native"
import { GestureHandlerRootView } from "react-native-gesture-handler"
import { SafeAreaView } from "react-native-safe-area-context"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Text } from "@/components/ui/text"

import {
  AssetOrigin,
  ScreenshotDetail,
  ScreenshotDetailRef,
  AnalysisResult,
} from "./ScreenshotDetail"

interface GalleryViewProps {
  onBack: () => void
  onRequestAnalyze?: (id: string, max_edge?: number) => void
  analysisResults?: Record<string, AnalysisResult>
  isAnalyzingMap?: Record<string, boolean>
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
        contentFit="cover"
        transition={200}
        cachePolicy="memory-disk"
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

    for (const asset of assets) {
      currentRowItems.push(asset)
      currentRowAspectRatio += asset.width / asset.height

      const estimatedWidth = currentRowAspectRatio * TARGET_ROW_HEIGHT
      const heightToMatchWidth =
        (containerWidth - currentRowItems.length * SPACING) / currentRowAspectRatio

      if (estimatedWidth >= containerWidth) {
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

export function GalleryView({
  onBack,
  onRequestAnalyze,
  analysisResults,
  isAnalyzingMap,
}: GalleryViewProps) {
  const [permissionResponse, requestPermission] = MediaLibrary.usePermissions()
  const [assets, setAssets] = useState<MediaLibrary.Asset[]>([])
  const [selectedAsset, setSelectedAsset] = useState<MediaLibrary.Asset | null>(null)
  const [selectedAssetOrigin, setSelectedAssetOrigin] = useState<AssetOrigin | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [hasNoAlbum, setHasNoAlbum] = useState(false)
  const [endCursor, setEndCursor] = useState<string | null>(null)
  const [hasNextPage, setHasNextPage] = useState(true)

  const screenshotDetailRef = useRef<ScreenshotDetailRef>(null)
  const [searchQuery, setSearchQuery] = useState("")
  const flatListRef = useRef<FlatList>(null)
  const itemRefs = useRef<Map<string, View>>(new Map()).current
  const [assetInfoMap, setAssetInfoMap] = useState<Record<string, MediaLibrary.AssetInfo>>({})

  const ALBUM_NAME = "RemoteRG"
  const screenWidth = Dimensions.get("window").width

  const filteredAssets = useMemo(() => {
    if (!searchQuery) return assets
    const query = searchQuery.toLowerCase()
    return assets.filter((asset) => asset.filename.toLowerCase().includes(query))
  }, [assets, searchQuery])

  const justifiedRows = useJustifiedLayout(filteredAssets, screenWidth)

  const loadAssets = useCallback(
    async (cursor?: string | null) => {
      if (permissionResponse?.status !== "granted") {
        return
      }

      if (cursor) {
        setLoadingMore(true)
      } else {
        setIsLoading(true)
        setHasNoAlbum(false)
      }

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
            first: 50,
            after: cursor || undefined,
          })

          if (cursor) {
            setAssets((prev) => [...prev, ...result.assets])
          } else {
            setAssets(result.assets)
          }

          setHasNextPage(result.hasNextPage)
          setEndCursor(result.endCursor)
        }
      } catch (e) {
        console.error("Failed to load assets", e)
      } finally {
        setIsLoading(false)
        setLoadingMore(false)
      }
    },
    [permissionResponse],
  )

  useEffect(() => {
    if (permissionResponse?.status === "granted") {
      loadAssets(null)
    }
  }, [permissionResponse, loadAssets])

  const handleLoadMore = () => {
    if (!loadingMore && hasNextPage && endCursor && !searchQuery) {
      loadAssets(endCursor)
    }
  }

  const handleAssetSelect = useCallback(
    (asset: MediaLibrary.Asset, origin: AssetOrigin) => {
      setSelectedAssetOrigin(origin)
      setSelectedAsset(asset)

      // Prefetch info if not exists
      if (!assetInfoMap[asset.id]) {
        MediaLibrary.getAssetInfoAsync(asset).then((info) => {
          setAssetInfoMap((prev) => ({ ...prev, [asset.id]: info }))
        })
      }
    },
    [assetInfoMap],
  )

  const getAssetOrigin = useCallback(async (id: string): Promise<AssetOrigin | null> => {
    const view = itemRefs.get(id)
    if (!view) return null

    return new Promise((resolve) => {
      view.measureInWindow((x, y, width, height) => {
        resolve({ x, y, width, height })
      })
    })
  }, [])

  const scrollToAsset = useCallback(
    (index: number) => {
      let rowIndex = 0

      for (let i = 0; i < justifiedRows.length; i++) {
        const row = justifiedRows[i]
        const rowHasItem = row.items.some((item) => item.asset.id === assets[index].id)
        if (rowHasItem) {
          rowIndex = i
          break
        }
      }

      flatListRef.current?.scrollToIndex({
        index: rowIndex,
        animated: false,
        viewPosition: 0.5,
      })
    },
    [justifiedRows, assets],
  )

  const handleBack = () => {
    if (selectedAsset) {
      if (screenshotDetailRef.current) {
        screenshotDetailRef.current.close()
      } else {
        setSelectedAsset(null)
      }
    } else {
      onBack()
    }
  }

  if (!permissionResponse) {
    return <View className="flex-1 bg-zinc-950" />
  }

  if (permissionResponse.status !== "granted") {
    return (
      <View className="flex-1 items-center justify-center bg-zinc-950 p-4">
        <Text className="text-white mb-4 text-center">アルバムへのアクセス権限が必要です。</Text>
        <Button onPress={requestPermission}>
          <Text>権限を許可する</Text>
        </Button>
        <Button variant="ghost" className="mt-4" onPress={onBack}>
          <Text className="text-zinc-400">戻る</Text>
        </Button>
      </View>
    )
  }

  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <View className="flex-1 bg-zinc-950">
        <StatusBar hidden />
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
            {isLoading ? (
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
            ) : filteredAssets.length === 0 ? (
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
                initialNumToRender={5}
                maxToRenderPerBatch={5}
                windowSize={5}
                onEndReached={handleLoadMore}
                onEndReachedThreshold={0.5}
                ListFooterComponent={
                  loadingMore ? (
                    <View className="py-4">
                      <ActivityIndicator size="small" color="#a855f7" />
                    </View>
                  ) : null
                }
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
            onRequestAnalyze={onRequestAnalyze}
            analysisResults={analysisResults}
            isAnalyzingMap={isAnalyzingMap}
            assetInfoMap={assetInfoMap}
            onAssetInfoLoaded={(id, info) => setAssetInfoMap((prev) => ({ ...prev, [id]: info }))}
          />
        )}
      </View>
    </GestureHandlerRootView>
  )
}
