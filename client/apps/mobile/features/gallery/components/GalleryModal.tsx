import { useRef, useEffect } from "react"
import {
  Modal,
  View,
  Dimensions,
  Animated,
  Easing,
} from "react-native"

import { AnalysisResult } from "../types"
import { GalleryView } from "./GalleryView"

interface GalleryModalProps {
  visible: boolean
  onClose: () => void
  onRequestAnalyze?: (id: string, max_edge?: number) => void
  analysisResults?: Record<string, AnalysisResult>
  isAnalyzingMap?: Record<string, boolean>
}

export function GalleryModal({
  visible,
  onClose,
  onRequestAnalyze,
  analysisResults,
  isAnalyzingMap,
}: GalleryModalProps) {
  const slideAnim = useRef(new Animated.Value(Dimensions.get("window").width)).current

  useEffect(() => {
    if (visible) {
      slideAnim.setValue(Dimensions.get("window").width)
      Animated.timing(slideAnim, {
        toValue: 0,
        duration: 300,
        useNativeDriver: true,
        easing: Easing.out(Easing.poly(4)),
      }).start()
    }
  }, [visible])

  const handleClose = () => {
    Animated.timing(slideAnim, {
      toValue: Dimensions.get("window").width,
      duration: 250,
      useNativeDriver: true,
      easing: Easing.in(Easing.poly(4)),
    }).start(() => {
      onClose()
    })
  }

  return (
    <Modal
      visible={visible}
      animationType="none"
      transparent={true}
      onRequestClose={handleClose}
      statusBarTranslucent
    >
      <View style={{ flex: 1, backgroundColor: "rgba(0,0,0,0.5)" }}>
        <Animated.View
          style={{
            transform: [{ translateX: slideAnim }],
            flex: 1,
          }}
        >
          <GalleryView
            onBack={handleClose}
            onRequestAnalyze={onRequestAnalyze}
            analysisResults={analysisResults}
            isAnalyzingMap={isAnalyzingMap}
          />
        </Animated.View>
      </View>
    </Modal>
  )
}
