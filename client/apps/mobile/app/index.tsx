import { useState, useEffect, useRef, useCallback } from "react";
import { View, TextInput, Button, StyleSheet, Dimensions, Text } from "react-native";
import {
  RTCPeerConnection,
  RTCIceCandidate,
  RTCSessionDescription,
  RTCView,
  mediaDevices,
} from 'react-native-webrtc';

const SIGNALING_URL_BASE = "ws://10.0.2.2:3000/api/signal";

export default function Index() {
  const [sessionId, setSessionId] = useState("fixed");
  const [isConnected, setIsConnected] = useState(false);
  // @ts-ignore
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
      console.log("Track received:", event.streams.length);
      if (event.streams && event.streams[0]) {
        console.log("Stream found at index 0");
        setRemoteStream(event.streams[0]);
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

  return (
    <View style={styles.container}>
      {isConnected && remoteStream ? (
        <View style={styles.videoContainer}>
            {/* @ts-ignore: stream prop is valid in v111+ but types might be lagging */}
            <RTCView
              stream={remoteStream}
              style={styles.video}
              objectFit="contain" 
            />
            <Button title="Disconnect" onPress={disconnect} color="red" />
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
  videoContainer: {
    flex: 1,
    justifyContent: 'center',
  },
  video: {
    width: Dimensions.get('window').width,
    height: Dimensions.get('window').height - 100, // Leave space for button
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

