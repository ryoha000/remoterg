import { View, StyleSheet, TouchableWithoutFeedback } from "react-native";
import { useState, useCallback } from "react";
import { VideoPlayer } from "./components/VideoPlayer";
import { ConnectForm } from "./components/ConnectForm";
import { ViewerOverlay } from "./components/ViewerOverlay";
import { useViewer } from "./hooks/useViewer";
import { StatusBar } from "expo-status-bar";

export function ViewerScreen() {
  const {
    sessionId,
    setSessionId,
    isConnected,
    remoteStream,
    status,
    connect,
    disconnect,
    rotate
  } = useViewer();

  const [showOverlay, setShowOverlay] = useState(true);

  const toggleOverlay = useCallback(() => {
    setShowOverlay(prev => !prev);
  }, []);

  return (
    <View style={styles.container}>
      <StatusBar hidden={isConnected && !showOverlay} />
      {isConnected && remoteStream ? (
        <View style={styles.videoContainer}>
            <VideoPlayer stream={remoteStream} onTap={toggleOverlay} />
            <ViewerOverlay 
              visible={showOverlay}
              status={status}
              onDisconnect={disconnect}
              onRotate={rotate}
            />
        </View>

      ) : (
        <ConnectForm 
          sessionId={sessionId}
          setSessionId={setSessionId}
          status={status}
          connect={connect}
        />
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#000',
  },
  videoContainer: {
    flex: 1,
    justifyContent: 'center',
    width: '100%',
    height: '100%',
  },
});
