import { StyleSheet } from "react-native";
import { RTCView, MediaStream } from 'react-native-webrtc';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, { useSharedValue, useAnimatedStyle, withTiming } from 'react-native-reanimated';

interface VideoPlayerProps {
  stream: MediaStream;
  onTap?: () => void;
}

export const VideoPlayer = ({ stream, onTap }: VideoPlayerProps) => {
  const scale = useSharedValue(1);
  const savedScale = useSharedValue(1);
  const translateX = useSharedValue(0);
  const savedTranslateX = useSharedValue(0);
  const translateY = useSharedValue(0);
  const savedTranslateY = useSharedValue(0);

  const pinchGesture = Gesture.Pinch()
    .onUpdate((e) => {
      scale.value = Math.max(1, savedScale.value * e.scale);
    })
    .onEnd(() => {
      savedScale.value = scale.value;
      if (scale.value < 1) {
          scale.value = withTiming(1);
          savedScale.value = 1;
      }
    });

  const panGesture = Gesture.Pan()
    .onUpdate((e) => {
      if (scale.value > 1) {
        translateX.value = savedTranslateX.value + e.translationX;
        translateY.value = savedTranslateY.value + e.translationY;
      }
    })
    .onEnd(() => {
       savedTranslateX.value = translateX.value;
       savedTranslateY.value = translateY.value;
    });

  const doubleTapGesture = Gesture.Tap()
    .numberOfTaps(2)
    .onEnd(() => {
      scale.value = withTiming(1);
      savedScale.value = 1;
      translateX.value = withTiming(0);
      savedTranslateX.value = 0;
      translateY.value = withTiming(0);
      savedTranslateY.value = 0;
    });

  const singleTapGesture = Gesture.Tap()
    .onEnd(() => {
        if (onTap) {
            // runOnJS is needed because this runs in UI thread
            // But we can try simple call first or use runOnJS from reanimated if generic worklet issue arises.
            // onTap is passed from props, so it should be fine if called from JS thread, 
            // but gesture callbacks form gesture-handler might be on UI thread.
            // Let's assume standard behavior for now, or use runOnJS if needed.
             // @ts-ignore
             if (global.runOnJS) global.runOnJS(onTap)(); else onTap();
        }
    })
    .requireExternalGestureToFail(doubleTapGesture);

  const animatedStyle = useAnimatedStyle(() => ({
    transform: [
      { translateX: translateX.value },
      { translateY: translateY.value },
      { scale: scale.value },
    ],
  }));

  const composed = Gesture.Simultaneous(pinchGesture, panGesture, doubleTapGesture, singleTapGesture);

  return (
    <GestureDetector gesture={composed}>
      <Animated.View style={[styles.video, animatedStyle, { overflow: 'hidden' }]}>
        {/* @ts-ignore: handling both props for compatibility */}
        <RTCView
          key={stream.toURL()} 
          streamURL={stream.toURL()} // @ts-ignore
          style={StyleSheet.absoluteFill}
          objectFit="contain"
        />
      </Animated.View>
    </GestureDetector>
  );
};

const styles = StyleSheet.create({
  video: {
    flex: 1,
    width: '100%',
    backgroundColor: '#333',
  },
});
