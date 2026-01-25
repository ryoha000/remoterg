import { Effect } from "effect";

// Helper interface for video element with non-standard captureStream
interface VideoElementWithCapture extends HTMLVideoElement {
  captureStream?(): MediaStream;
  mozCaptureStream?(): MediaStream;
}

export const createMockVideoElement = () => {
  const vid = document.createElement("video");
  vid.src = "/mock.mp4";
  vid.loop = true;
  vid.muted = true;
  // vid.playsInline = true; // Not strictly needed for properties but good for mocks
  vid.setAttribute("playsinline", "");
  vid.crossOrigin = "anonymous";
  vid.style.position = "absolute";
  vid.style.top = "-9999px";
  vid.style.left = "-9999px";
  document.body.appendChild(vid);
  void vid.play();
  return vid;
};

export class MockWebSocket extends EventTarget {
  readyState: number = WebSocket.CONNECTING;
  url: string;

  constructor(url: string) {
    super();
    this.url = url;
    setTimeout(() => {
      this.readyState = WebSocket.OPEN;
      this.dispatchEvent(new Event("open"));
    }, 100);
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    if (typeof data === "string") {
      try {
        const msg = JSON.parse(data);
        if (msg.type === "offer") {
          setTimeout(() => {
            const answer = {
              type: "answer",
              sdp: "mock-answer-sdp",
            };
            this.dispatchEvent(
              new MessageEvent("message", { data: JSON.stringify(answer) }),
            );
          }, 100);
        }
      } catch {
        // ignore
      }
    }
  }

  close() {
    this.readyState = WebSocket.CLOSED;
    this.dispatchEvent(new Event("close"));
  }
}

export class MockRTCDataChannel extends EventTarget {
  readyState: RTCDataChannelState = "connecting";
  label: string;
  ordered: boolean;

  constructor(label: string, options?: RTCDataChannelInit) {
    super();
    this.label = label;
    this.ordered = options?.ordered ?? true;
    setTimeout(() => {
      this.readyState = "open";
      this.dispatchEvent(new Event("open"));
    }, 50);
  }

  send(_data: string | Blob | ArrayBuffer | ArrayBufferView): void {
    // console.log("MockDataChannel send:", data);
  }

  close() {
    this.readyState = "closed";
    this.dispatchEvent(new Event("close"));
  }
}

export class MockRTCPeerConnection extends EventTarget {
  connectionState: RTCPeerConnectionState = "new";
  iceConnectionState: RTCIceConnectionState = "new";
  localDescription: RTCSessionDescription | null = null;
  remoteDescription: RTCSessionDescription | null = null;
  videoElement: HTMLVideoElement | null = null;
  transceivers: RTCRtpTransceiver[] = [];

  constructor(_config: RTCConfiguration) {
    super();
  }

  createDataChannel(label: string, options?: RTCDataChannelInit): RTCDataChannel {
    return new MockRTCDataChannel(label, options) as unknown as RTCDataChannel;
  }

  createOffer(): Promise<RTCSessionDescriptionInit> {
    return Promise.resolve({ type: "offer", sdp: "mock-offer-sdp" });
  }

  setLocalDescription(desc: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = desc as RTCSessionDescription;
    return Promise.resolve();
  }

  setRemoteDescription(desc: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = desc as RTCSessionDescription;
    if (desc.type === "answer") {
      // simulate connection success
      this.simulateConnection();
    }
    return Promise.resolve();
  }

  addIceCandidate(_candidate: RTCIceCandidateInit): Promise<void> {
    return Promise.resolve();
  }

  addTransceiver(
    trackOrKind: MediaStreamTrack | string,
    _init?: RTCRtpTransceiverInit,
  ): RTCRtpTransceiver {
    const transceiver = {
      receiver: { track: { kind: typeof trackOrKind === "string" ? trackOrKind : trackOrKind.kind } },
      setCodecPreferences: () => {},
    } as unknown as RTCRtpTransceiver;
    this.transceivers.push(transceiver);
    return transceiver;
  }

  getTransceivers() {
    return this.transceivers;
  }

  close() {
    this.connectionState = "closed";
    if (this.videoElement) {
      this.videoElement.pause();
      this.videoElement.remove();
      this.videoElement = null;
    }
  }

  private simulateConnection() {
    this.connectionState = "connected";
    this.iceConnectionState = "connected";
    this.dispatchEvent(new Event("connectionstatechange"));
    this.dispatchEvent(new Event("iceconnectionstatechange"));

    // Emit track
    this.videoElement = createMockVideoElement();
    let stream: MediaStream | null = null;
    const v = this.videoElement as VideoElementWithCapture;
    if (v.captureStream) stream = v.captureStream();
    else if (v.mozCaptureStream) stream = v.mozCaptureStream();

    if (stream) {
      const track = stream.getVideoTracks()[0];
      const event = new RTCTrackEvent("track", {
        track: track,
        receiver: { track } as RTCRtpReceiver,
        streams: [stream],
        transceiver: {} as RTCRtpTransceiver,
      });
      this.dispatchEvent(event);
    }
  }
}

export const createMockPeerConnection = (config: RTCConfiguration) =>
  new MockRTCPeerConnection(config) as unknown as RTCPeerConnection;

export const createMockWebSocket = (url: string) =>
  new MockWebSocket(url) as unknown as WebSocket;
