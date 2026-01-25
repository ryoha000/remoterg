import { useState, useEffect, useRef, useCallback } from "react";
import { View, TextInput, Button, StyleSheet, Dimensions, Text } from "react-native";
import * as ScreenOrientation from 'expo-screen-orientation';
import {
  RTCPeerConnection,
  RTCIceCandidate,
  RTCSessionDescription,
  RTCView,
  mediaDevices,
  MediaStream,
} from 'react-native-webrtc';

const SIGNALING_URL_BASE = "ws://10.0.2.2:3000/api/signal";

export default function Index() {
  const [sessionId, setSessionId] = useState("fixed");
  const [isConnected, setIsConnected] = useState(false);
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null);
  const [status, setStatus] = useState("Disconnected");
  
  const wsRef = useRef<WebSocket | null>(null);
  const pcRef = useRef<RTCPeerConnection | null>(null);

  const connect = useCallback(() => {
    setStatus("Connecting...");
    
    // 1. WebSocket Setup
    const wsUrl = `${SIGNALING_URL_BASE}?session_id=${sessionId}&role=viewer`;
    console.log("Connecting WS:", wsUrl);
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = async () => {
      setStatus("WS Open. Creating PC...");
      setupPeerConnection(ws);
    };

    ws.onmessage = async (event) => {
      const msg = JSON.parse(event.data);
      console.log("WS Message:", msg.type);

      if (!pcRef.current) return;

      try {
        if (msg.type === "answer") {
          await pcRef.current.setRemoteDescription(new RTCSessionDescription({ type: "answer", sdp: msg.sdp }));
          setStatus("Remote Description Set");
        } else if (msg.type === "ice_candidate") {
          const candidate = new RTCIceCandidate({
            candidate: msg.candidate,
            sdpMid: msg.sdp_mid,
            sdpMLineIndex: msg.sdp_mline_index,
          });
          await pcRef.current.addIceCandidate(candidate);
          console.log("Added ICE Candidate");
        }
      } catch (err) {
        console.error("Signaling Error:", err);
      }
    };

    ws.onerror = (e) => {
      console.error("WS Error:", e);
      setStatus("WS Error");
    };

    ws.onclose = () => {
      console.log("WS Closed");
      setStatus("Disconnected (WS Closed)");
      setIsConnected(false);
    };

  }, [sessionId]);

  const setupPeerConnection = async (ws: WebSocket) => {
    // 2. PeerConnection Config
    const config = {
      iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }],
    };

    const pc = new RTCPeerConnection(config);
    pcRef.current = pc;

    // Monitor Connection State
    // @ts-ignore
    pc.onconnectionstatechange = () => {
      console.log("PC Connection State:", pc.connectionState);
      setStatus(`PC: ${pc.connectionState}`);
      if (pc.connectionState === "connected") {
        setIsConnected(true);
      }
    };

    // @ts-ignore
    pc.oniceconnectionstatechange = () => {
      console.log("ICE Connection State:", pc.iceConnectionState);
    };

    // Handle ICE Candidates
    // @ts-ignore
    pc.onicecandidate = (event: any) => {
      if (event.candidate) {
        ws.send(JSON.stringify({
          type: "ice_candidate",
          candidate: event.candidate.candidate,
          sdp_mid: event.candidate.sdpMid,
          sdp_mline_index: event.candidate.sdpMLineIndex,
        }));
      }
    };

    // Handle Tracks (Video)
    // @ts-ignore: react-native-webrtc types might be slightly off or I'm lazy with the event type
    pc.ontrack = (event: any) => {
      const track = event.track;
      console.log("Track received:", track?.kind, track?.id);
      
      if (track && track.kind === "video") { // Only care about video for RTCView
        track.enabled = true;
        setRemoteStream((prev) => {
          // Create new stream with ONLY this video track
          const newStream = new MediaStream(undefined); 
          newStream.addTrack(track);
          
          console.log(`Created new Video Stream: ${newStream.toURL()} with track ${track.id} (${track.kind}) state:${track.readyState}`);
          return newStream;
        });
      } else if (track) {
         console.log(`Ignoring non-video track for RTCView: ${track.kind} ${track.id}`);
         track.enabled = true; // Still enable audio, it plays automatically via PC
      }
    };

    // 3. Add Transceivers (RecvOnly)
    pc.addTransceiver("video", { direction: "recvonly" });
    pc.addTransceiver("audio", { direction: "recvonly" });

    // 4. Create Data Channel (Required by hostd?)
    console.log("Creating DataChannel...");
    const dc = pc.createDataChannel("data");
    // @ts-ignore
    dc.onopen = () => console.log("DataChannel Open");
    // @ts-ignore
    dc.onmessage = (e: any) => console.log("DC Message:", e.data);


    // 5. Create Offer
    try {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      console.log("Sending Offer...");
      
      ws.send(JSON.stringify({
        type: "offer",
        sdp: offer.sdp,
        codec: "h264" // Default to h264 as per common config
      }));
    } catch (err) {
      console.error("PC Setup Error:", err);
      setStatus("Error creating offer");
    }
  };


  const disconnect = () => {
    if (wsRef.current) wsRef.current.close();
    if (pcRef.current) pcRef.current.close();
    wsRef.current = null;
    pcRef.current = null;
    setIsConnected(false);
    setRemoteStream(null);
    setStatus("Disconnected");
  };

  // Cleanup on unmount
  useEffect(() => {
    return () => disconnect();
  }, []);

  // Stats Logging
  useEffect(() => {
    if (!isConnected || !pcRef.current) return;

    const interval = setInterval(async () => {
      const pc = pcRef.current;
      if (!pc) return;

      try {
        // @ts-ignore
        const stats = await pc.getStats();
        // @ts-ignore
        stats.forEach((report) => {
          if (report.type === 'inbound-rtp' && report.kind === 'video') {
             console.log(`[Video Stats] Bytes: ${report.bytesReceived}, Packets: ${report.packetsReceived}, Decoded: ${report.framesDecoded}, Dropped: ${report.framesDropped}, Lost: ${report.packetsLost}`);
          }
        });
      } catch (e) {
        console.error("Stats logging error:", e);
      }
    }, 2000);

    return () => clearInterval(interval);
  }, [isConnected]);

  useEffect(() => {
    // Unlock orientation to allow user to rotate device nicely
    ScreenOrientation.unlockAsync();
  }, []);

  const rotate = async () => {
    const current = await ScreenOrientation.getOrientationAsync();
    if (current === ScreenOrientation.Orientation.PORTRAIT_UP || current === ScreenOrientation.Orientation.PORTRAIT_DOWN) {
      await ScreenOrientation.lockAsync(ScreenOrientation.OrientationLock.LANDSCAPE_LEFT);
    } else {
      await ScreenOrientation.lockAsync(ScreenOrientation.OrientationLock.PORTRAIT_UP);
    }
  };

  return (
    <View style={styles.container}>
      {isConnected && remoteStream ? (
        <View style={styles.videoContainer}>
            {/* @ts-ignore: handling both props for compatibility */}
            <RTCView
              key={remoteStream.toURL()} 
              streamURL={remoteStream.toURL()}
              style={styles.video}
              objectFit="contain"
            />
            <View style={styles.controls}>
               <Button title="Rotate" onPress={rotate} />
               <Button title="Disconnect" onPress={disconnect} color="red" />
            </View>
        </View>

      ) : (
        <View style={styles.formContainer}>
          <Text style={styles.title}>RemoteRG Mobile</Text>
          <Text style={styles.status}>{status}</Text>
          <Text style={styles.label}>Session ID</Text>
          <TextInput
            style={styles.input}
            value={sessionId}
            onChangeText={setSessionId}
            placeholder="Enter Session ID"
            autoCapitalize="none"
          />
          <Button title="Connect" onPress={connect} />
        </View>
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
  },
  video: {
    flex: 1,
    width: '100%',
    backgroundColor: '#333',
  },
  formContainer: {
    flex: 1,
    justifyContent: 'center',
    padding: 20,
    backgroundColor: '#fff',
  },
  title: {
    fontSize: 24,
    fontWeight: 'bold',
    marginBottom: 20,
    textAlign: 'center',
  },
  status: {
    marginBottom: 20,
    textAlign: 'center',
    color: 'gray',
  },
  label: {
    marginBottom: 5,
    fontWeight: 'bold',
  },
  input: {
    borderWidth: 1,
    borderColor: '#ccc',
    padding: 10,
    marginBottom: 20,
    borderRadius: 5,
  },
});

