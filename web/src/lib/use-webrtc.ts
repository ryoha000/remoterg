import { useRef, useState, useCallback, useEffect } from 'react';

export interface WebRTCOptions {
  signalUrl: string;
  sessionId: string;
  codec?: 'h264' | 'any';
  onTrack?: (stream: MediaStream) => void;
  onConnectionStateChange?: (state: string) => void;
  onIceConnectionStateChange?: (state: string) => void;
}

export interface WebRTCStats {
  inbound?: {
    bytesReceived?: number;
    framesReceived?: number;
    packetsLost?: number;
  };
  track?: {
    framesDecoded?: number;
    framesDropped?: number;
    freezeCount?: number;
  };
}

interface RTCInboundRtpStats {
  type: string;
  kind?: string;
  bytesReceived?: number;
  framesReceived?: number;
  packetsLost?: number;
}

interface RTCTrackStats {
  type: string;
  framesDecoded?: number;
  framesDropped?: number;
  freezeCount?: number;
}

function isRTCInboundRtpStats(report: unknown): report is RTCInboundRtpStats {
  if (typeof report !== 'object' || report === null) {
    return false;
  }
  const r = report as Record<string, unknown>;
  return r.type === 'inbound-rtp';
}

function isRTCTrackStats(report: unknown): report is RTCTrackStats {
  if (typeof report !== 'object' || report === null) {
    return false;
  }
  const r = report as Record<string, unknown>;
  return r.type === 'track';
}

export function useWebRTC(options: WebRTCOptions) {
  const {
    signalUrl,
    sessionId,
    codec = 'h264',
    onTrack,
    onConnectionStateChange,
    onIceConnectionStateChange,
  } = options;

  const [connectionState, setConnectionState] = useState<string>('disconnected');
  const [iceConnectionState, setIceConnectionState] = useState<string>('new');
  const [stats, setStats] = useState<WebRTCStats>({});
  const [logs, setLogs] = useState<Array<{ time: string; message: string; type: string }>>([]);

  const pcRef = useRef<RTCPeerConnection | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const dataChannelRef = useRef<RTCDataChannel | null>(null);
  const statsIntervalRef = useRef<number | null>(null);
  const pendingIceCandidatesRef = useRef<Array<RTCIceCandidateInit>>([]);
  const remoteDescSetRef = useRef<boolean>(false);
  const mediaStreamRef = useRef<MediaStream | null>(null);

  const addLog = useCallback((message: string, type: string = 'info') => {
    const time = new Date().toLocaleTimeString();
    setLogs((prev) => [...prev, { time, message, type }]);
  }, []);

  const connect = useCallback(async () => {
    try {
      // 前回の接続のMediaStreamをクリア
      if (mediaStreamRef.current) {
        mediaStreamRef.current.getTracks().forEach(track => track.stop());
        mediaStreamRef.current = null;
      }

      setConnectionState('connecting');
      addLog('WebSocket接続を開始...');

      // WebSocket接続
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const wsUrl = `${protocol}//${window.location.host}${signalUrl}?session_id=${sessionId}&role=viewer`;
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        addLog('WebSocket接続確立', 'success');
        createPeerConnection();
      };

      ws.onmessage = async (event) => {
        try {
          const message = JSON.parse(event.data);
          addLog(`受信: ${message.type}`);

          if (message.type === 'error') {
            addLog(`サーバーエラー: ${message.message}`, 'error');
            setConnectionState('error');
            ws.close();
          } else if (message.type === 'answer') {
            try {
              addLog(`Answer SDP (最初の100文字): ${message.sdp.substring(0, 100)}...`);
              if (pcRef.current) {
                await pcRef.current.setRemoteDescription({
                  type: 'answer',
                  sdp: message.sdp,
                });
                remoteDescSetRef.current = true;
                addLog('Answerを設定', 'success');

                // Answer適用後に滞留ICE候補をまとめて適用
                while (pendingIceCandidatesRef.current.length > 0) {
                  const candidate = pendingIceCandidatesRef.current.shift();
                  if (candidate && pcRef.current) {
                    try {
                      await pcRef.current.addIceCandidate(candidate);
                      addLog('ICE candidate追加（バッファから）', 'success');
                    } catch (error) {
                      addLog(`ICE candidate追加エラー（バッファ）: ${error}`, 'error');
                    }
                  }
                }
              }
            } catch (error) {
              addLog(`Answer設定エラー: ${error}`, 'error');
            }
          } else if (message.type === 'ice_candidate') {
            try {
              const candidateInit: RTCIceCandidateInit = {
                candidate: message.candidate,
                sdpMid: message.sdp_mid,
                sdpMLineIndex: message.sdp_mline_index,
              };

              if (!remoteDescSetRef.current) {
                pendingIceCandidatesRef.current.push(candidateInit);
                addLog('ICE candidateをバッファに保存（Answer待ち）', 'info');
              } else if (pcRef.current) {
                await pcRef.current.addIceCandidate(candidateInit);
                addLog('ICE candidate追加', 'success');
              }
            } catch (error) {
              addLog(`ICE candidate追加エラー: ${error}`, 'error');
            }
          }
        } catch (error) {
          addLog(`メッセージ処理エラー: ${error}`, 'error');
        }
      };

      ws.onerror = (error) => {
        addLog(`WebSocketエラー: ${error}`, 'error');
        setConnectionState('error');
      };

      ws.onclose = () => {
        addLog('WebSocket接続が閉じられました', 'error');
        setConnectionState('disconnected');
      };
    } catch (error) {
      addLog(`接続エラー: ${error}`, 'error');
      setConnectionState('error');
    }
  }, [signalUrl, sessionId, addLog]);

  const createPeerConnection = useCallback(() => {
    addLog('PeerConnectionを作成...');

    const pc = new RTCPeerConnection({
      iceServers: [],
    });
    pcRef.current = pc;

    pc.onicecandidate = (event) => {
      if (event.candidate) {
        if (!wsRef.current) {
          addLog('ICE candidate: WebSocket参照がnullです', 'error');
          return;
        }

        const wsState = wsRef.current.readyState;
        if (wsState === WebSocket.OPEN) {
          try {
            const iceMessage = {
              type: 'ice_candidate',
              candidate: event.candidate.candidate,
              sdp_mid: event.candidate.sdpMid,
              sdp_mline_index: event.candidate.sdpMLineIndex,
            };
            wsRef.current.send(JSON.stringify(iceMessage));
            addLog(`ICE candidate送信: ${event.candidate.candidate.substring(0, 50)}...`);
          } catch (sendError) {
            addLog(`ICE candidate送信エラー: ${sendError}`, 'error');
            console.error('ICE candidate送信エラー詳細:', sendError);
          }
        } else {
          addLog(`ICE candidate: WebSocketがOPEN状態ではありません (state: ${wsState})`, 'error');
        }
      }
    };

    pc.ontrack = async (event) => {
      addLog(`ストリームを受信 (tracks=${event.streams?.[0]?.getTracks().length ?? 0})`, 'success');
      if (event.track) {
        addLog(`トラック情報 kind=${event.track.kind} id=${event.track.id} readyState=${event.track.readyState}`);
      }

      // MediaStreamを再利用して、すべてのトラックを1つのストリームに集約
      if (!mediaStreamRef.current) {
        mediaStreamRef.current = new MediaStream();
        addLog('新しいMediaStreamを作成', 'success');
      }

      // 受信したトラックを既存のストリームに追加
      if (event.track) {
        mediaStreamRef.current.addTrack(event.track);
        addLog(`トラックを追加: kind=${event.track.kind}, total tracks=${mediaStreamRef.current.getTracks().length}`, 'success');

        // ストリームが更新されたことをコールバックで通知
        onTrack?.(mediaStreamRef.current);
      }
    };

    pc.onconnectionstatechange = () => {
      const state = pc.connectionState;
      addLog(`接続状態: ${state}`);
      setConnectionState(state);
      onConnectionStateChange?.(state);

      if (state === 'failed' || state === 'closed') {
        stopStatsLogging();
      } else if (state === 'connected') {
        startStatsLogging();
      }
    };

    pc.oniceconnectionstatechange = () => {
      const iceState = pc.iceConnectionState;
      addLog(`ICE接続状態: ${iceState}`);
      setIceConnectionState(iceState);
      onIceConnectionStateChange?.(iceState);

      if (iceState === 'connected' || iceState === 'completed') {
        startStatsLogging();
      } else if (iceState === 'failed' || iceState === 'disconnected') {
        stopStatsLogging();
      }
    };

    pc.onicegatheringstatechange = () => {
      addLog(`ICE収集状態: ${pc.iceGatheringState}`);
    };

    pc.onsignalingstatechange = () => {
      addLog(`シグナリング状態: ${pc.signalingState}`);
    };

    // Video受信用のtransceiverを追加（recvonly）
    const videoTransceiver = pc.addTransceiver('video', { direction: 'recvonly' });
    addLog('Video recvonly transceiverを追加', 'success');

    // Audio受信用のtransceiverを追加（recvonly）
    pc.addTransceiver('audio', { direction: 'recvonly' });
    addLog('Audio recvonly transceiverを追加', 'success');

    // codec指定
    if (codec === 'h264') {
      try {
        const capabilities = RTCRtpSender.getCapabilities('video');
        const codecs = (capabilities?.codecs ?? []).filter(
          (c) =>
            c.mimeType === 'video/H264' &&
            (c.sdpFmtpLine ?? '').includes('packetization-mode=1')
        );
        if (codecs.length > 0 && typeof videoTransceiver.setCodecPreferences === 'function') {
          videoTransceiver.setCodecPreferences(codecs);
          addLog(`H.264 codec preferenceを適用 (${codecs.length}件)`, 'success');
        }
      } catch (error) {
        addLog(`codec preference設定エラー: ${error}`, 'error');
      }
    }

    // DataChannelを作成
    const dataChannel = pc.createDataChannel('input', { ordered: true });
    dataChannelRef.current = dataChannel;
    dataChannel.onopen = () => {
      addLog('DataChannelが開きました', 'success');
    };
    dataChannel.onerror = (error) => {
      addLog(`DataChannelエラー: ${error}`, 'error');
    };

    // Offerを作成して送信
    pc.createOffer()
      .then(async (offer) => {
        try {
          await pc.setLocalDescription(offer);
          addLog('Offerを作成しました', 'success');
          addLog(`Offer SDP (最初の100文字): ${offer.sdp?.substring(0, 100)}...`);

          // WebSocket状態を確認
          if (!wsRef.current) {
            addLog('WebSocket参照がnullです。Offerを送信できません。', 'error');
            return;
          }

          const wsState = wsRef.current.readyState;
          const wsStateText = 
            wsState === WebSocket.CONNECTING ? 'CONNECTING' :
            wsState === WebSocket.OPEN ? 'OPEN' :
            wsState === WebSocket.CLOSING ? 'CLOSING' :
            wsState === WebSocket.CLOSED ? 'CLOSED' : 'UNKNOWN';
          
          addLog(`WebSocket状態確認: ${wsStateText} (${wsState})`);

          if (wsState === WebSocket.OPEN) {
            try {
              const offerMessage = {
                type: 'offer',
                sdp: offer.sdp,
                codec: codec,
              };
              wsRef.current.send(JSON.stringify(offerMessage));
              addLog('OfferをWebSocketで送信しました', 'success');
              addLog(`送信メッセージ: type=${offerMessage.type}, codec=${offerMessage.codec}, sdp_length=${offer.sdp?.length ?? 0}`);
            } catch (sendError) {
              addLog(`Offer送信エラー: ${sendError}`, 'error');
              console.error('Offer送信エラー詳細:', sendError);
            }
          } else {
            addLog(`WebSocketがOPEN状態ではありません (${wsStateText})。Offerを送信できません。`, 'error');
          }
        } catch (error) {
          addLog(`LocalDescription設定エラー: ${error}`, 'error');
          console.error('LocalDescription設定エラー詳細:', error);
        }
      })
      .catch((error) => {
        addLog(`Offer作成エラー: ${error}`, 'error');
        console.error('Offer作成エラー詳細:', error);
      });
  }, [codec, addLog, onTrack, onConnectionStateChange, onIceConnectionStateChange]);

  const startStatsLogging = useCallback(() => {
    if (!pcRef.current || statsIntervalRef.current !== null) return;

    const receiver = pcRef.current.getReceivers().find((r) => r.track?.kind === 'video');
    if (!receiver) {
      addLog('stats: videoレシーバーが見つかりませんでした');
      return;
    }

    statsIntervalRef.current = window.setInterval(async () => {
      try {
        const reports = await receiver.getStats();
        let inboundStats: WebRTCStats['inbound'] = undefined;
        let trackStats: WebRTCStats['track'] = undefined;

        reports.forEach((report) => {
          if (isRTCInboundRtpStats(report) && report.kind === 'video') {
            inboundStats = {
              bytesReceived: report.bytesReceived,
              framesReceived: report.framesReceived,
              packetsLost: report.packetsLost,
            };
          } else if (isRTCTrackStats(report)) {
            trackStats = {
              framesDecoded: report.framesDecoded,
              framesDropped: report.framesDropped,
              freezeCount: report.freezeCount,
            };
          }
        });

        setStats({
          inbound: inboundStats,
          track: trackStats,
        });
      } catch (err) {
        addLog(`stats取得エラー: ${err}`, 'error');
      }
    }, 2000);
  }, [addLog]);

  const stopStatsLogging = useCallback(() => {
    if (statsIntervalRef.current !== null) {
      clearInterval(statsIntervalRef.current);
      statsIntervalRef.current = null;
    }
  }, []);

  const sendKey = useCallback((key: string, down: boolean = true) => {
    if (dataChannelRef.current && dataChannelRef.current.readyState === 'open') {
      dataChannelRef.current.send(
        JSON.stringify({
          type: 'key',
          key: key,
          down: down,
        })
      );
      addLog(`キー送信: ${key} (down: ${down})`);
    } else {
      addLog('DataChannelが開いていません', 'error');
    }
  }, [addLog]);

  const requestScreenshot = useCallback(() => {
    if (dataChannelRef.current && dataChannelRef.current.readyState === 'open') {
      dataChannelRef.current.send(
        JSON.stringify({
          type: 'screenshot_request',
        })
      );
      addLog('スクリーンショットリクエスト送信');
    } else {
      addLog('DataChannelが開いていません', 'error');
    }
  }, [addLog]);

  const disconnect = useCallback(() => {
    stopStatsLogging();
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    if (pcRef.current) {
      pcRef.current.close();
      pcRef.current = null;
    }
    // MediaStreamをクリア
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach(track => track.stop());
      mediaStreamRef.current = null;
    }
    setConnectionState('disconnected');
    addLog('接続を切断しました');
  }, [stopStatsLogging, addLog]);

  useEffect(() => {
    return () => {
      disconnect();
    };
  }, [disconnect]);

  return {
    connectionState,
    iceConnectionState,
    stats,
    logs,
    connect,
    disconnect,
    sendKey,
    requestScreenshot,
  };
}


