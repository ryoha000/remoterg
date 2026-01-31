import * as MediaLibrary from "expo-media-library"
import { StatusBar } from "expo-status-bar"
import { forwardRef, useImperativeHandle } from "react"
import { View } from "react-native"
import { FlatList } from "react-native-gesture-handler"
import Animated, { useAnimatedStyle, interpolate } from "react-native-reanimated"
import { SafeAreaView } from "react-native-safe-area-context"

import { useScreenshotDetail } from "../hooks/useScreenshotDetail"
import { AnalysisResult, AssetOrigin } from "../types"
import { ScreenshotDetailActions } from "./ScreenshotDetailActions"
import { ScreenshotDetailHeader } from "./ScreenshotDetailHeader"
import { ScreenshotDetailInfoPanel } from "./ScreenshotDetailInfoPanel"
import { ScreenshotPage } from "./ScreenshotPage"

const INFO_PANEL_WIDTH_PCT = 0.35

export interface ScreenshotDetailRef {
  close: () => void
}

interface ScreenshotDetailProps {
  assets: MediaLibrary.Asset[]
  initialIndex: number
  origin?: AssetOrigin | null
  onClose: () => void
  getAssetOrigin: (id: string) => Promise<AssetOrigin | null>
  onCurrentIndexChange: (index: number) => void
  onRequestAnalyze?: (id: string, max_edge?: number) => void
  analysisResults?: Record<string, AnalysisResult>
  isAnalyzingMap?: Record<string, boolean>
  assetInfoMap: Record<string, MediaLibrary.AssetInfo>
  onAssetInfoLoaded: (id: string, info: MediaLibrary.AssetInfo) => void
  onDelete?: (id: string) => Promise<void>
}

export const ScreenshotDetail = forwardRef<ScreenshotDetailRef, ScreenshotDetailProps>(
  (props, ref) => {
    const {
      currentIndex,
      currentAsset,
      currentAssetInfo,
      isClosing,
      closingOrigin,
      anim,
      showInfo,
      infoAnim,
      windowDimensions,
      hostId,
      dbHostInfo,
      analysis,
      isAnalyzing,
      handleClose,
      handleDelete,
      handleTwitterShare,
      handleGenericShare,
      toggleInfo,
      onViewableItemsChanged,
    } = useScreenshotDetail(props)

    useImperativeHandle(ref, () => ({
      close: handleClose,
    }))

    // Backdrop opacity
    const backdropStyle = useAnimatedStyle(() => ({
      opacity: anim.value,
    }))

    // Info Panel Animation
    const infoPanelStyle = useAnimatedStyle(() => {
      const width = windowDimensions.width * INFO_PANEL_WIDTH_PCT
      const translateX = interpolate(infoAnim.value, [0, 1], [width, 0])
      // Also fade
      const opacity = interpolate(infoAnim.value, [0, 0.5, 1], [0, 0, 1])

      return {
        transform: [{ translateX }],
        opacity,
        width,
      }
    })

    // Header/Bottom Gradient Animation
    const overlayControlsStyle = useAnimatedStyle(() => ({
      opacity: infoAnim.value,
      pointerEvents: infoAnim.value > 0.5 ? "auto" : "none",
      right: interpolate(
        infoAnim.value,
        [0, 1],
        [0, windowDimensions.width * INFO_PANEL_WIDTH_PCT],
      ),
    }))

    if (!currentAsset) return null

    return (
      <View className="flex-1 bg-transparent absolute inset-0 z-50">
        <StatusBar hidden={!showInfo} />
        {/* Dark Backdrop */}
        <Animated.View
          style={[{ flex: 1, backgroundColor: "#000000" }, backdropStyle]}
          className="absolute inset-0"
        />

        {/* Carousel */}
        <FlatList
          data={props.assets}
          horizontal
          pagingEnabled
          initialScrollIndex={props.initialIndex}
          getItemLayout={(data, index) => ({
            length: windowDimensions.width,
            offset: windowDimensions.width * index,
            index,
          })}
          showsHorizontalScrollIndicator={false}
          onViewableItemsChanged={onViewableItemsChanged}
          viewabilityConfig={{ itemVisiblePercentThreshold: 50 }}
          scrollEnabled={!isClosing}
          extraData={props.assetInfoMap}
          renderItem={({ item, index }) => {
            const isCurrent = index === currentIndex
            const info = props.assetInfoMap[item.id]
            const uri = info?.localUri || info?.uri

            return (
              <ScreenshotPage
                asset={item}
                isActive={isCurrent}
                windowDimensions={windowDimensions}
                onTap={toggleInfo}
                onDismiss={handleClose}
                shouldAnimateEntry={index === props.initialIndex}
                origin={index === props.initialIndex ? props.origin : null}
                isClosing={isClosing}
                closingOrigin={closingOrigin}
                isInfoOpen={showInfo}
                uri={uri}
              />
            )
          }}
          keyExtractor={(item) => item.id}
        />

        {/* Overlays */}
        <SafeAreaView
          className="flex-1 absolute inset-0 pointer-events-box-none"
          edges={["top", "bottom", "left", "right"]}
          pointerEvents="box-none"
          style={{ zIndex: 20 }}
        >
          <ScreenshotDetailHeader onClose={handleClose} style={overlayControlsStyle} />

          <ScreenshotDetailInfoPanel
            currentAsset={currentAsset}
            currentAssetInfo={currentAssetInfo}
            analysis={analysis}
            isAnalyzing={isAnalyzing}
            hostId={hostId}
            onRequestAnalyze={props.onRequestAnalyze}
            style={infoPanelStyle}
            dbHostInfo={dbHostInfo}
          />

          <ScreenshotDetailActions
            onTwitterShare={handleTwitterShare}
            onGenericShare={handleGenericShare}
            onDelete={handleDelete}
            style={overlayControlsStyle}
          />
        </SafeAreaView>
      </View>
    )
  },
)
