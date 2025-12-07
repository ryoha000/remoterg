## 現象
- WebUI では接続状態・ICE/DTLS ともに `connected`/`completed` だが、`stats inbound-rtp` が常に無しで video が表示されない。
- WebRTC internals では transport で bytes/packets の増加は見えるが、video inbound RTP エントリが現れない。

## 環境・前提
- ホスト: `cargo run --bin hostd`（dummy frame generator、OpenH264 で赤フレーム送出想定）
- ブラウザ: localhost:8080 の WebUI（H.264 のみ優先、recvonly）
- 通信は同一マシン内想定（NAT/防火壁なし）

## これまで試したこと
1. WebUI 側
   - `ontrack` ログ強化（track.kind/id）、`video.play()` リトライ。
   - `getStats` で inbound-rtp/track の定期ログ、requestVideoFrameCallback でフレーム到達確認ログ追加。
   - 選択候補ペアを `getStats` からロギング（nominated/succeeded を一度だけ出力）。
   - `?codec=any` パラメータで H.264 固定を外せるようにした（デフォルトは H.264 優先）。
2. 送信側（Rust webrtc サービス）
   - Offer/Answer SDP を info ログ出力。
   - H.264 PT を 103 に固定（Offer 先頭 PT と合わせた）。
   - 送出フレーム数を 5 秒毎に info ログ。
   - `SettingEngine` で loopback candidate を有効化（`set_include_loopback_candidate(true)`）。
   - 非 loopback フィルタは削除（候補送出自体は全て許可）。
3. ICE 候補・経路の確認
   - webrtc-internals の candidate grid で、nominated ペアは prflx(local 61413) ⇔ host 127.0.0.1:61417 などが選択。
   - bytesSent/bytesReceived が transport/candidate-pair では増えるが、inbound video RTP エントリは出ず、WebUI `stats inbound-rtpなし` 継続。

## ログ所見（代表例）
- Host ログ: フレーム送出カウンタは 5 秒毎に 60〜70 程度で増加し続ける（送信ループは動作）。
- Answer SDP: `m=video 9 UDP/TLS/RTP/SAVPF 103`、`fmtp:103 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f`。
- Offer SDP: `m=video 9 UDP/TLS/RTP/SAVPF 103 109 117 119`（ブラウザ側 H.264 優先）。
- webrtc-internals:
  - `selectedCandidatePairId` が prflx(local 61413) ⇔ host(127.0.0.1:63188/63190) などで succeeded/nominated。
  - transport: packetsSent/Received は 10〜20 程度カウントされるが、inbound RTP track が存在しない（framesDecoded/framesReceived が見当たらない）。

## 対応と結果
- 追加ログの意図
  - 送出経路のどこで詰まっているかを可視化するため、以下を 5 秒周期で出力:
    - `RTCRtpSender::get_parameters()`: SSRC/Codec/PT を確認し、m-line/PT の不整合や SSRC 未設定を検知。
    - `RTCRtpSender::get_stats()` (OutboundRTP 相当): `bytes_sent/packets_sent` が増えるかで送信実体が動いているかを判定。
    - `transceiver direction/current_direction`: m-line 無効化や方向不一致（Sendonly/Recvonly/Inactive）を確認。
- 追加ログで判明したこと
  - `OutboundRTP` が `get_stats` に出てこない → RTP パケットが SRTP writer まで流れていない。
  - transceiver[0] は `mid=0` で Sendonly/Sendonly、m-line 無効化ではない。
  - 余剰 transceiver[1] が `mid=None` で未交渉状態だが直接の原因ではなさそう。
- 原因の見立て
  - `RTCRtpSender::send()` が走らず Track が bind されておらず、packetizer/Sequencer 未初期化 → `write_sample` は静かに drop され OutboundRTP も生成されない。
  - （webrtc-rs の `start_rtp_senders` に依存しており、交渉状況によっては send 呼び出しが漏れた可能性）
- 実施した対策
  - `RTCRtpSender::get_parameters()`/`get_stats()`/`transceiver direction` を 5 秒周期でログし、送出パイプラインの詰まり箇所を可視化。
  - `set_local_description` 直後に `RTCRtpSender::send()` を明示呼び出し（既に送信済みなら debug ログのみ）し、送信開始を確実化。
  - その結果、OutboundRTP が生成されブラウザ側 inbound-rtp が現れ、映像表示が復旧。

## 既知の改善余地
- 余剰 transceiver（mid None, direction Sendrecv）が残るため、必要に応じて整理する。
- コーデック切替（VP8）による切り分けは未実施だが、現状 H.264 で動作確認済み。

## 用語メモ（WebRTCに詳しくない方向け）
- PeerConnection (PC): ブラウザとメディアをやり取りする接続オブジェクト。ICE/DTLS/SRTP の制御もここに集約される。
- ICE: 通信経路を探す仕組み。候補（candidate）の組み合わせを試し、最終的に選ばれたペアでメディアを送る。
- DTLS: 鍵交換と暗号化のための TLS。これが確立すると SRTP（実データの暗号化）が開始できる。
- SRTP: 実際の音声・映像パケットを暗号化して送るプロトコル。RTP を暗号化したもの。
- m-line / mid: SDP 内のメディア行。`mid` はメディア行の識別子で、どの track/transceiver と対応するかを示す。
- transceiver: 送受のペア（sender と receiver）。`direction` が Sendonly/Recvonly/Sendrecv/Inactive で役割を決める。
- SSRC (Synchronization Source): RTP ストリームを一意に識別する 32bit ID。送受双方でこの値を鍵にストリームを区別する。SDP/params ログで SSRC が設定されているかを必ず確認。
- Codec: 映像/音声の符号化方式（例: H.264, VP8）。SDP の `a=rtpmap` などで合意し、双方が同じ codec でエンコード/デコードする。
- PT (Payload Type): RTP ヘッダに載る 7bit の番号。どの codec で符号化されているかを示す。SDP で PT と codec が対応付けられ、送信側は指定 PT で送り、受信側は PT を見て正しいデコーダを選択する。
- RTP sender stats (OutboundRTP): 送信パケット数・バイト数など。これが 0 のままなら実パケットが出ていない可能性が高い。
- inbound-rtp (ブラウザ側): 受信側でのパケット/フレーム統計。ここが増えればブラウザが受け取れている。
