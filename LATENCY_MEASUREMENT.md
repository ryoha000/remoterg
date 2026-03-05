# RemoteRG E2E レイテンシ計測ガイド（現行実装準拠）

関連ドキュメント: [LATENCY_MEASUREMENT_ARCHITECTURE.md](LATENCY_MEASUREMENT_ARCHITECTURE.md)

## 目的
- `hostd` と Android 間で時計が一致していない前提でも、映像 E2E レイテンシを継続計測する
- 実装済みコードと 1:1 で対応する運用手順を記述する

## スコープ
- 本書は **映像 E2E** の現行実装のみを対象とする
- Input-to-Photon は現時点で未実装（本書の対象外）
- 集計（P50/P95/P99）のバッチ出力は別途運用（オンライン表示の主経路は本書記載）

## 前提
- `server` は signaling server ではなく `hostd`
- 時刻同期は WebRTC DataChannel で行う
- E2E 算出の時刻基準は monotonic clock を使用
  - hostd: `Instant` 基準の monotonic ms
  - Android: `SystemClock.elapsedRealtimeNanos()` 基準の monotonic ms

## 全体方針（現行）
1. DataChannel の NTP 風往復で `offsetMonoMs`（hostd monotonic → Android monotonic の補正）を推定
2. hostd から `frame_sample`（`t_cap` など）を送信
3. Android JNI Sink で `VideoFrame.packet_infos().absolute_capture_time` を読んで `captureUnixMs` と `timestampUs` を取得
4. `frame_sample` と native callback を `capture_unix_ms` 完全一致で突合
5. 突合済み `timestampUs` と render 側 `timestampUs` を完全一致で突合
6. `t_cap_client_mono = t_cap_hostd_mono - offsetMonoMs`、`E2E = t_render_client_mono - t_cap_client_mono`

## 1. DataChannel メッセージ仕様（実装実体）

### 1.1 sync_req
```json
{
  "sync_req": {
    "seq": 42,
    "c1": 12345.678
  }
}
```

### 1.2 sync_res
```json
{
  "sync_res": {
    "seq": 42,
    "c1": 12345.678,
    "s2": 56789.012,
    "s3": 56789.045
  }
}
```

### 1.3 frame_sample
```json
{
  "frame_sample": {
    "seq": 1001,
    "frame_id": 56789,
    "t_cap": 14567.101,
    "t_enc_in": 14567.109,
    "t_enc_out": 14567.122,
    "t_send": 14567.124,
    "capture_unix_ms": 1772708904306
  }
}
```

補足:
- `c1/c4`: Android monotonic ms
- `s2/s3`: hostd monotonic ms
- `capture_unix_ms`: `t_cap` を hostd 側で壁時計に変換した値（突合キー用途）

## 2. 時刻同期推定（offsetMonoMs）

### 2.1 推定式
- `rtt = (c4 - c1) - (s3 - s2)`
- `offsetMonoMs = ((s2 - c1) + (s3 - c4)) / 2`

### 2.2 運用ルール（現行実装）
- `sync_req` は 5 秒ごとに送信
- サンプル履歴は最大 100 件保持
- `rtt` が小さい順に上位 25% を採用
- 採用サンプルの `offsetMonoMs` 中央値を算出
- 平滑化: `offset_est = alpha * latest + (1 - alpha) * old`（`alpha = 0.1`）

## 3. hostd 側実装

### 3.1 採取時刻
- `t_cap`: キャプチャ直後
- `t_enc_in`: エンコード投入時
- `t_enc_out`: エンコード出力時
- `t_send`: `frame_sample` 送信時

### 3.2 RTP abs-capture-time 拡張
- video RTP に abs-capture-time 拡張を付与
- 現行は 16 byte 拡張（`absolute_capture_timestamp` + `estimated_capture_clock_offset`）
- `estimated_capture_clock_offset` は sender/capturer 同時計前提で `0` を設定

## 4. Android 側 E2E 算出フロー（C 経路）

### 4.1 native callback（JNI Sink）
- C++ Sink で `VideoFrame.packet_infos()` を走査し `absolute_capture_time` を抽出
- `onCaptureTime(status, captureUnixMs, timestampUs)` を毎フレーム callback
- `status`:
  - `0`: ok
  - `1`: no_packet_infos
  - `2`: no_abs_capture_time
  - `3`: out_of_range

### 4.2 3点結合
1. `frame_sample` と native callback を `capture_unix_ms` 完全一致で突合
2. 突合結果から `t_cap_hostd_mono` と `timestampUs` を確定
3. `t_cap_client_mono = t_cap_hostd_mono - offsetMonoMs` に変換
4. render 側の `timestampUs` と完全一致突合
5. `E2E = t_render_client_mono - t_cap_client_mono`

### 4.3 ストア仕様
- `FrameNativeMatchStore`: key=`capture_unix_ms`、TTL=1000ms
- `NativeRenderMatchStore`: key=`timestampUs`、TTL=1000ms
- どちらも先着側を保持し、後着時に即マッチ

## 5. 表示値更新ポリシー
- `lastLatencyMs` は **C 経路でマッチ成功したフレームのみ**更新
- `E2E < 0` は異常値として警告ログのみ（値更新しない）
- C 経路が未成立のフレームは「前回値を維持」
- DataChannel `frame_sample` 単独（旧 B 経路）では表示値を更新しない

## 6. ログ運用
- 主なタグ:
  - `Latency[sync]`
  - `Latency[frame_sample]`
  - `Latency[C-native]`
  - `Latency[C]`
  - `Latency[C-miss]`
  - `Latency[C-native-skip]`
  - `Latency[C-evict]`
  - `Latency[C-clear]`
- C++ 側:
  - `ACT frame=...`（抽出成功）
  - `ACT skip ... status=...`（抽出失敗）

## 7. 既知の制約
- `capture_unix_ms` は突合キー用途であり、最終 E2E 計算は monotonic 基準で行う
- `frame_sample` と native callback の到着順は保証されないため、TTL 内での待ち合わせが前提
- decode 内訳（`t_recv`, `t_decode_out`）は現行主経路では未採用

## 8. テスト（現行）
- `LatencyMonotonicMathTest`
  - RTT/offset 式
  - `hostd mono -> client mono` 変換
  - median/smoothing
- `FrameNativeMatchStoreTest`
  - frame 先着 / native 先着
  - TTL eviction
