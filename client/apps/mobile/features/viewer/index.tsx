import { View, Button, StyleSheet } from "react-native";
import { VideoPlayer } from "./components/VideoPlayer";
import { ConnectForm } from "./components/ConnectForm";
import { useViewer } from "./hooks/useViewer";

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

  return (
    <View style={styles.container}>
      {isConnected && remoteStream ? (
        <View style={styles.videoContainer}>
            <VideoPlayer stream={remoteStream} />
            <View style={styles.controls}>
               <Button title="Rotate" onPress={rotate} />
               <Button title="Disconnect" onPress={disconnect} color="red" />
            </View>
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
  controls: {
    position: 'absolute',
    bottom: 20,
    right: 20,
    flexDirection: 'row',
    gap: 10,
    zIndex: 100, // Ensure buttons are clickable
  },
  videoContainer: {
    flex: 1,
    justifyContent: 'center',
    width: '100%',
    height: '100%',
  },
});
