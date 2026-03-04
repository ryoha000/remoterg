# RemoteRG レイテンシ測定ガイド

## 目的
- 別デバイス間で時計が一致していない状況でも、`hostd` とAndroidアプリ間のレイテンシを実用精度で測定する
- 映像系（E2E）と入力系（Input-to-Photon）を分離して評価する
- 記事で再現可能な手順として公開できる形にする

## 前提
- 計測対象の `server` は signaling server ではなく `hostd` を指す
- 時刻同期は WebRTC の DataChannel 上で行う
- 時計は OS の壁時計ではなく monotonic clock を使う
  - Rust: `std::time::Instant` 相当の単調増加時刻
  - Android(Kotlin): `SystemClock.elapsedRealtimeNanos()`（単位はns、集計時にmsへ変換）

## 方針
- Androidアプリと `hostd` の時刻オフセットを NTP 風に推定
- 推定したオフセットで `hostd` 側タイムスタンプをAndroid時刻系へ変換
- 変換後に E2E / Input-to-Photon を算出
- 1回の推定ではなく複数回サンプリングし、低RTTサンプル中心で安定化

## 1. 時刻同期プロトコル（DataChannel）

### 1.1 メッセージ仕様（最小）
```json
{
  "type": "sync_req",
  "seq": 42,
  "c1": 12345.678
}
```

```json
{
  "type": "sync_res",
  "seq": 42,
  "c1": 12345.678,
  "s2": 56789.012,
  "s3": 56789.045
}
```

- `c1`: client send time（Android monotonic）
- `s2`: server receive time（hostd monotonic）
- `s3`: server send time（hostd monotonic）
- `c4`: client receive time（Android monotonic, 受信時にローカルで採取）

### 1.2 推定式
- `rtt = (c4 - c1) - (s3 - s2)`
- `offset = ((s2 - c1) + (s3 - c4)) / 2`

解釈:
- `offset` は「Android時刻に対するserver時刻のずれ」
- Android時刻系に変換したい場合:
  - `server_ts_in_client = server_ts - offset`

## 2. オフセット推定の実運用ルール
- 1回ではなく 30-100 サンプル取得
- `rtt` が小さい順に上位 20-30% を採用
- 採用サンプルの `offset` の中央値を採用
- 実験中は定期再推定（例: 5秒ごと）
- 急な変動対策で平滑化:
  - `offset_est = alpha * new + (1 - alpha) * old`
  - `alpha` の初期値は `0.1` 程度

## 3. 映像系レイテンシ（E2E）測定

### 3.1 収集する時刻
- `t_cap`（hostd, キャプチャ直後）
- `t_enc_in`（hostd）
- `t_enc_out`（hostd）
- `t_send`（hostd）
- `t_recv`（Android, RTPフレーム受信時刻またはフレーム到着時刻）
- `t_decode_out`（Android, デコード済みフレーム取得時刻）
- `t_render`（Android, 表示に最も近い時刻）

### 3.1.1 Androidでの `t_render` 取得方針
- 最低限（実装容易）:
  - WebRTCの `VideoSink` 受信時点で `elapsedRealtimeNanos()` を取得し `t_render_proxy` として記録
  - これは厳密な表示時刻ではなく「表示直前」に近い時刻
- 推奨（より厳密）:
  - `SurfaceTexture` / `TextureView` 更新コールバックで時刻取得
  - 必要に応じて `Choreographer` のVSync時刻と組み合わせる
- 記事では、どのレベルの `t_render` を採用したかを明記する

### 3.1.2 Androidでの decode時間取得方針
- 取得できる場合（厳密）:
  - デコーダの入力/出力コールバックから `t_packet_recv` と `t_decode_out` を取得
  - `DecodeLatency = t_decode_out - t_packet_recv`
- 取得が難しい場合（proxy）:
  - RTP/フレーム受信時点を `t_recv`
  - `VideoSink` でデコード済みフレームを受けた時刻を `t_decode_out`
  - `DecodeProxy = t_decode_out - t_recv`
- 記事では「厳密値」か「proxy値」かを必ず明記する

### 3.2 算出
- `t_cap_client = t_cap - offset_est`
- `E2E = t_render - t_cap_client`

補助指標:
- `EncodeLatency = t_enc_out - t_enc_in`
- `DecodeLatency = t_decode_out - t_packet_recv`（取得可能時）
- `DecodeProxy = t_decode_out - t_recv`（取得困難時の代替）
- `ReadbackCost ~= t_enc_in - t_cap`（GPU->CPU readback 比較で使用）

## 4. 入力系レイテンシ（Input-to-Photon）測定

### 4.1 収集する時刻
- `t_input_send`（Android, 入力送信時）
- `t_input_recv`（hostd, 入力受信時）
- `t_input_applied_frame_cap`（hostd, 入力効果が初めて現れたフレームの `t_cap`）
- `t_render`（Android, 当該フレーム描画時）

### 4.2 算出
- `t_input_recv_client = t_input_recv - offset_est`
- `t_input_applied_frame_cap_client = t_input_applied_frame_cap - offset_est`
- `InputToPhoton = t_render - t_input_send`

分解:
- `Uplink = t_input_recv_client - t_input_send`
- `ApplyWait = t_input_applied_frame_cap_client - t_input_recv_client`
- `RenderAfterApply = t_render - t_input_applied_frame_cap_client`

## 5. ログフォーマット（推奨）

### 5.1 syncログ
```json
{
  "kind": "sync_sample",
  "seq": 42,
  "c1": 12345.678,
  "s2": 56789.012,
  "s3": 56789.045,
  "c4": 12345.912,
  "rtt": 0.201,
  "offset": 44443.233
}
```

### 5.2 frameログ
```json
{
  "kind": "frame_sample",
  "frame_id": 12345,
  "t_cap": 56790.100,
  "t_enc_in": 56790.110,
  "t_enc_out": 56790.122,
  "t_send": 56790.123,
  "t_recv": 12346.430,
  "t_decode_out": 12346.470,
  "t_render": 12346.500,
  "offset_est": 44443.230
}
```

### 5.3 inputログ
```json
{
  "kind": "input_sample",
  "input_id": 987,
  "t_input_send": 12347.001,
  "t_input_recv": 56790.920,
  "t_input_applied_frame_cap": 56790.940,
  "t_render": 12347.120,
  "offset_est": 44443.230
}
```

## 6. 集計ルール
- 各条件 3-5 run、1 run 60秒以上
- 指標は平均でなく `P50/P95/P99` を主に掲載
- 実験条件（HW/SW、Codec、キュー戦略、readback有無）は 1変数ずつ変更
- 出力:
  - E2E `P50/P95/P99`
  - Input-to-Photon `P50/P95/P99`
  - DecodeProxy または DecodeLatency `P50/P95/P99`
  - `CPU%`（hostd/Androidアプリ）
  - drop率

## 7. よくある落とし穴
- 壁時計（`System.currentTimeMillis()`）を混ぜる
- 1サンプルだけで offset を決める
- RTTが大きいサンプルを採用してしまう
- `t_render` を「受信時刻」で代用する（描画時刻で取る）
- decode proxy と厳密decode時間を混同する
- 映像系と入力系の評価軸を混在させる

## 8. 妥当性確認（おすすめ）
- 少数回だけ物理計測（高速撮影）を行い、推定値のオーダーを照合
- 推定値と物理値に大きな乖離がないか確認
- 記事では「主計測はNTP風補正、物理計測は妥当性確認」と明記

## 9. 記事記載テンプレ（短文）
- 本検証では、DataChannel を用いた NTP 風オフセット推定により、`hostd` とAndroidアプリの単調増加時刻を同一時間軸へ正規化した。  
- オフセットは複数サンプルから低RTT区間を抽出して中央値を採用し、E2E、Input-to-Photon、decode指標を `P50/P95/P99` で評価した。  
- 絶対値の誤差を抑えるため、比較は同一環境で1変数ずつ変更して実施した。
