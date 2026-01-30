import { useState, useEffect, useMemo, useRef } from "react"
import { Alert, Platform, Dimensions } from "react-native"
import * as FileSystem from "expo-file-system/legacy"
import * as MediaLibrary from "expo-media-library"
import * as Sharing from "expo-sharing"
import Share, { Social } from "react-native-share"
import { useSharedValue, withTiming, Easing, runOnJS } from "react-native-reanimated"
import { useSafeAreaInsets } from "react-native-safe-area-context"

import { useAnalysis } from "@/db/queries/use-analysis"
import { useHostId } from "@/db/queries/use-screenshots"

import { AnalysisResult, AssetOrigin } from "../types"

interface UseScreenshotDetailProps {
  assets: MediaLibrary.Asset[]
  initialIndex: number
  origin?: AssetOrigin | null
  onClose: () => void
  getAssetOrigin: (id: string) => Promise<AssetOrigin | null>
  onCurrentIndexChange: (index: number) => void
  analysisResults?: Record<string, AnalysisResult>
  isAnalyzingMap?: Record<string, boolean>
  assetInfoMap: Record<string, MediaLibrary.AssetInfo>
  onAssetInfoLoaded: (id: string, info: MediaLibrary.AssetInfo) => void
  onDelete?: (id: string) => Promise<void>
}

export const useScreenshotDetail = ({
  assets,
  initialIndex,
  onClose,
  getAssetOrigin,
  onCurrentIndexChange,
  analysisResults,
  isAnalyzingMap,
  assetInfoMap,
  onAssetInfoLoaded,
  onDelete,
}: UseScreenshotDetailProps) => {
  const [currentIndex, setCurrentIndex] = useState(initialIndex)

  // Closing state
  const [isClosing, setIsClosing] = useState(false)
  const [closingOrigin, setClosingOrigin] = useState<AssetOrigin | null>(null)

  // Animation state: 0 = closed (at thumbnail), 1 = open (fullscreen)
  // We use this primarily for the backdrop opacity now
  const anim = useSharedValue(0)

  // Info/Overlay state: true = Info Visible, false = Fullscreen
  const [showInfo, setShowInfo] = useState(false)
  // Default to false (Fullscreen) as per "Initially object-fit: contain" request

  // Animated value for toggling UI
  const infoAnim = useSharedValue(0)

  const windowDimensions = Dimensions.get("window")
  const insets = useSafeAreaInsets()

  // Safety: Clamp index if assets change (e.g. deletion)
  useEffect(() => {
    if (currentIndex >= assets.length && assets.length > 0) {
      setCurrentIndex(assets.length - 1)
    }
  }, [assets.length, currentIndex])

  const currentAsset = assets[currentIndex]
  const currentAssetInfo = currentAsset ? assetInfoMap[currentAsset.id] : undefined

  // Load persisted analysis & Resolve Host ID
  const { data: dbHostInfo } = useHostId(currentAsset?.id ?? "")
  const hostId = useMemo(() => {
    if (!currentAsset) return null
    if (dbHostInfo) return dbHostInfo.hostId
    return currentAsset.filename?.replace(/\.[^/.]+$/, "") || currentAsset.id
  }, [dbHostInfo, currentAsset])

  // Load analysis using Local ID
  const { data: savedAnalysis } = useAnalysis(currentAsset?.id ?? "")

  const sessionAnalysis = hostId ? analysisResults?.[hostId] : null
  const analysis = sessionAnalysis || savedAnalysis

  const isAnalyzing = (hostId ? isAnalyzingMap?.[hostId] : false) ?? false

  useEffect(() => {
    // Start global animation (backdrop)
    anim.value = withTiming(1, {
      duration: 350,
      easing: Easing.out(Easing.cubic),
    })
  }, [])

  useEffect(() => {
    const fetchInfo = async () => {
      // Fetch current and neighbors
      const indicesToFetch = [currentIndex, currentIndex - 1, currentIndex + 1].filter(
        (i) => i >= 0 && i < assets.length,
      )

      for (const i of indicesToFetch) {
        const asset = assets[i]
        if (!asset || assetInfoMap[asset.id]) continue

        try {
          const info = await MediaLibrary.getAssetInfoAsync(asset)
          if (info) {
            onAssetInfoLoaded(asset.id, info)
          }
        } catch (e) {
          console.error("Failed to load asset info for", asset.id, e)
        }
      }
    }
    fetchInfo()
  }, [currentIndex, assets, assetInfoMap, onAssetInfoLoaded])

  // Sync overlay animation with state
  useEffect(() => {
    infoAnim.value = withTiming(showInfo && !isClosing ? 1 : 0, {
      duration: 300,
      easing: Easing.inOut(Easing.cubic),
    })
  }, [showInfo, isClosing])

  const handleClose = async () => {
    // 1. Prepare closing
    setIsClosing(true)

    // 2. Hide overlay immediately (via effect)
    setShowInfo(false)

    // 3. Get target origin
    const currentAsset = assets[currentIndex]
    // Ensure we scroll to it first (though onCurrentIndexChange should have handled it mostly)
    onCurrentIndexChange(currentIndex)

    const origin = await getAssetOrigin(currentAsset.id)
    setClosingOrigin(origin)

    // 4. Fade out backdrop
    anim.value = withTiming(
      0,
      {
        duration: 350,
        easing: Easing.out(Easing.cubic),
      },
      (finished) => {
        if (finished) {
          runOnJS(onClose)()
        }
      },
    )
  }

  const handleDelete = async () => {
    if (onDelete && currentAsset) {
      await onDelete(currentAsset.id)
    }
  }

  const handleTwitterShare = async () => {
    try {
      if (!currentAsset) return
      const uri = currentAsset.uri
      if (!uri) return

      if (Platform.OS === "android") {
        const cacheDir = FileSystem.cacheDirectory
        if (!cacheDir) {
          throw new Error("Cache directory not available")
        }

        // 拡張子を元のファイルに合わせる（重要）
        const cacheFileUri = `${cacheDir}twitter-share-tmp.png`

        // 1. キャッシュディレクトリにコピー（既にある場合は上書き）
        await FileSystem.copyAsync({
          from: uri,
          to: cacheFileUri,
        })

        // 2. Base64に変換せず、ファイルURIをそのまま渡す
        // react-native-share は 'file://' 形式を期待しています
        await Share.shareSingle({
          title: "Share via Twitter",
          url: cacheFileUri, // 'data:image/...' ではなく 'file://...'
          type: "image/png", // 実際の形式に合わせる
          social: Social.Twitter,
        })
      } else {
        // iOS
        await Share.shareSingle({
          title: "Share via Twitter",
          url: uri,
          social: Social.Twitter,
        })
      }
    } catch (e: any) {
      console.error("Twitter share failed", e)
      Alert.alert("Share Error", `Failed to share: ${e.message || e}`)
    }
  }

  const handleGenericShare = async () => {
    if (!currentAsset) return
    const uri = currentAsset.uri
    if (uri) {
      await Sharing.shareAsync(uri)
    }
  }

  const toggleInfo = () => {
    setShowInfo((prev) => !prev)
  }

  const onViewableItemsChanged = useRef(({ viewableItems }: any) => {
    if (viewableItems.length > 0) {
      const newIndex = viewableItems[0].index
      setCurrentIndex(newIndex)
      if (newIndex !== null && newIndex !== undefined) {
        onCurrentIndexChange(newIndex)
      }
    }
  }).current

  return {
    currentIndex,
    currentAsset,
    currentAssetInfo,
    isClosing,
    closingOrigin,
    anim,
    showInfo,
    infoAnim,
    windowDimensions,
    insets,
    hostId,
    dbHostInfo: dbHostInfo ?? undefined,
    analysis,
    isAnalyzing,
    handleClose,
    handleDelete,
    handleTwitterShare,
    handleGenericShare,
    toggleInfo,
    onViewableItemsChanged,
    setCurrentIndex, // exposed if needed
  }
}
