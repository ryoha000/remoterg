# RemoteRG 実装状況メモ

最終更新: 2025年12月（VP8/VP9エンコードとDataChannel配線を追加）

## プロジェクト概要

自宅のWindows PC上で動いているノベルゲームを、スマホ／タブレットから映像付きでリモートプレイできる仕組みを開発中。

詳細仕様は [SPEC.md](SPEC.md) を参照。

## 現在の実装状況

### ✅ 実装完了 / 実装中

#### 1. ワークスペース構成・共有型
- Rustワークスペース（`desktop/services/`）に`core-types`（共有DTO）と`encoder`（映像エンコード）を追加
- 依存方針: 各サービスは`core-types`のみに依存し、組み立ては`hostd`のみが担当（サービス間直接依存なし）

#### 2. CaptureService
- **状態**: ダミーフレーム生成を強化
- **機能**:
  - 10色パレットの単色フレームを事前生成し、45fps想定でローテーション送出
  - 解像度・FPSの更新、開始/停止コマンドを受信して動的に切替
  - `tokio::mpsc`経由でフレームを配信
- **未実装**: Windows GraphicsCapture APIによる実キャプチャ

#### 3. Encoder クレート
- **状態**: 複数コーデック対応のワーカー型エンコーダーを実装
- **機能**:
  - OpenH264（0.9）によるH.264エンコード
  - libvpx（vpx-rs 0.2.1）によるVP8/VP9エンコード（I420変換込み）
  - 複数ワーカーを起動して結果を1つのチャネルに集約、初期数は`hostd`側で2
- **未実装**: ハードウェアエンコード

#### 4. WebRtcService
- **状態**: webrtc-rs 0.14 を用いた実装が完成
- **機能**:
  - Offer受信→実際のAnswer生成、ICE候補送受信
  - 受信コーデック指定を解釈し、利用可能な`encoder`ファクトリから VP9 > VP8 > H.264 の優先順で選択
  - TrackLocalStaticSampleで映像トラックを生成し、接続完了までフレームをドロップしてから送出
  - PLI/FIR 受信時のキーフレーム再送要求、RTCP/統計ログ
  - DataChannelを開き、JSONメッセージを`InputService`へ中継
- **既知**: 複数接続は未対応（グローバルチャネル前提）

#### 5. SignalingService
- **状態**: AxumベースのHTTP/WebSocketシグナリングサーバ
- **機能**:
  - `/` で `desktop/services/web/index.html` を配信、`/signal` で WebSocket
  - Offer/ICEをWebRtcServiceへ転送し、Answer/ICEをクライアントへ返却
  - グローバルbroadcastチャンネルで応答を配信（複数接続時は競合の可能性あり）

#### 6. Web UI（`desktop/services/web/index.html`）
- **状態**: デバッグ用シングルHTMLを拡充
- **機能**:
  - コーデック選択（VP8/VP9/H.264/自動）、Video recvonly transceiver
  - DataChannelでキー入力・スクリーンショット要求を送信
  - Answer適用前のICEバッファリング、接続状態/ICE状態/受信統計の詳細ログ
  - `requestVideoFrameCallback` と `getStats` で受信確認

#### 7. InputService
- **状態**: 受信メッセージをログ出力するスケルトン
- **未実装**: Win32 SendInputによる入力注入、スクリーンショット応答

#### 8. hostd（統合バイナリ）
- **状態**: 全サービスを束ねるCLIバイナリ
- **機能**:
  - `--port`, `--log-level` オプション
  - コーデックfeature（`vp9`/`vp8`/`h264`）をビルド時に選択、未指定ならコンパイルエラー
  - 起動直後にCaptureServiceへStartを送信し、全サービスを並列実行

### 📋 テスト状況

- ✅ CaptureService: ユニットテスト実装済み（2テスト）
- ✅ SignalingService: シリアライゼーションテスト実装済み
- ⚠️ 統合テスト・E2Eテスト: 未実装

### 🔧 ビルド・実行

```bash
# ビルド
cd desktop/services
cargo build --release

# 実行
cargo run --bin hostd

# オプション付き実行
cargo run --bin hostd -- --port 9000 --log-level debug
```

## 現在の制限事項・既知の問題

### 1. 複数接続対応が不完全
- **原因**: broadcastチャンネルを前提とした簡易実装
- **影響**: 複数クライアント同時接続時に応答の取り違えが起こり得る
- **対応**: 接続ID単位のチャンネル管理とPeerConnection管理を導入する

### 2. 実キャプチャ未実装
- **現状**: 単色ダミーフレームのみ（事前生成）
- **必要**: Windows GraphicsCapture API統合とリサイズ/フレームレート協調

### 3. 入力注入・スクリーンショット未実装
- **現状**: DataChannelメッセージはInputServiceでログ出力のみ
- **必要**: Win32 SendInputによるキー/マウス注入、スクリーンショット応答の実装

### 4. DataChannel/接続管理の粗さ
- **現状**: グローバルチャネル前提、単一接続向け。再接続・複数接続の切り替えを考慮していない
- **必要**: 接続ごとのDataChannelハンドラ分離、切断時のリソース解放

### 5. エンコードはソフトウェアのみ
- **現状**: OpenH264/libvpxのソフトウェア実装。CPU負荷が高くなる可能性
- **必要**: ハードウェアエンコード検討（NVENC/AMF/QuickSync等）と負荷計測

## 次のステップ（優先順位順）

### 短期（動作確認フェーズ）

1. **接続ごとのチャンネル管理の実装**
   - WebSocketごとに応答チャネル/PeerConnection/encoderワーカーを分離する
2. **GraphicsCapture統合**
   - HWND指定で実際のウィンドウをキャプチャし、解像度・FPS変更に追従させる
3. **DataChannel入力・スクリーンショット実装**
   - Win32 SendInputによるキー/マウス注入
   - スクリーンショット取得とクライアント返却
4. **エンコード負荷の検証**
   - CPU使用率計測、ハードウェアエンコード手段の調査

### 中期（機能実装フェーズ）

5. **複数コーデック/帯域に応じた設定最適化**
   - VP8/VP9/H.264 の選択ロジックやビットレート設定の最適化
6. **セッション管理の強化**
   - タイムアウト、切断処理、再接続時のクリーンアップ

### 長期（品質向上フェーズ）

7. **C# UI（WinUI）の実装**
   - IPCプロトコルの定義
   - UI実装と設定管理

8. **セキュリティ機能**
   - PINコード認証
   - Tailscale統合時のアクセス制御

9. **パフォーマンス最適化**
   - レイテンシ削減、メモリ使用量削減、ハードウェアエンコード対応

## アーキテクチャ

### サービス間通信

```
┌─────────────┐
│  C# UI      │
│  (未実装)   │
└──────┬──────┘
       │ IPC (JSON/TCP or Named Pipe)
       ↓
┌─────────────────────────────────────┐
│         hostd (統合バイナリ)         │
│                                     │
│  ┌──────────────┐                  │
│  │CaptureService│                  │
│  │  (ダミー)    │                  │
│  └──────┬───────┘                  │
│         │ Frame                    │
│         ↓                          │
│  ┌──────────────┐                  │
│  │WebRtcService │                  │
│  │ (webrtc-rs)  │                  │
│  └──────┬───────┘                  │
│         │ DataChannel (受信→ログ) │
│         ↓                          │
│  ┌──────────────┐                  │
│  │InputService  │                  │
│  │  (ログのみ)  │                  │
│  └──────────────┘                  │
│                                     │
│  ┌──────────────┐                  │
│  │SignalingService│                │
│  │  (実装済み)   │                  │
│  └──────┬───────┘                  │
│         │ broadcast channel        │
│         ↓                          │
│  ┌──────────────┐                  │
│  │WebRtcService │                  │
│  │  (応答送信)  │                  │
│  └──────────────┘                  │
└─────────────────────────────────────┘
       │ WebSocket
       ↓
┌─────────────┐
│ Web Browser │
│  (実装済み)  │
└─────────────┘
```

### メッセージフロー

1. **シグナリング**:
   ```
   ブラウザ → WebSocket → SignalingService → WebRtcService
   WebRtcService → broadcast channel → SignalingService → WebSocket → ブラウザ
   ```

2. **映像配信**:
   ```
   CaptureService → Frame → WebRtcService → VideoTrack → ブラウザ
   ```

3. **入力処理**:
   ```
   ブラウザ → DataChannel → WebRtcService → InputService → Win32 SendInput
   ```

## 技術スタック

### バックエンド（Rust）
- **ランタイム**: tokio (非同期)
- **HTTP/WebSocket**: axum
- **シリアライゼーション**: serde, serde_json
- **ログ**: tracing, tracing-subscriber
- **エラーハンドリング**: anyhow
- **CLI**: clap

### フロントエンド（Web）
- `desktop/services/web/index.html`: バニラJavaScript + WebRTCデバッグUI
- `web/`: TanStack Start / ReactベースのPoC（現状ホストとは未連携）

### 現在使用中
- **WebRTC**: webrtc-rs 0.14, webrtc-media 0.11
- **映像エンコード**: openh264 0.9（H.264）、vpx-rs 0.2.1（VP8/VP9）
- **共有DTO**: core-types クレート
- **バイト処理**: bytes 1.0

### 将来追加予定
- **Windows API**: windows-rs または winapi
- **C# UI**: WinUI 3

## ディレクトリ構造

```
remoterg/
├── SPEC.md                    # プロダクト仕様書
├── NOTE.md                    # このファイル
├── desktop/
│   ├── services/              # Rustワークスペース
│   │   ├── Cargo.toml        # ワークスペース設定
│   │   ├── README.md          # サービス詳細ドキュメント
│   │   ├── core/              # 共有DTO・型（core-types）
│   │   ├── encoder/           # 映像エンコード（H.264/VP8/VP9）
│   │   ├── capture/           # キャプチャサービス
│   │   ├── signaling/         # シグナリングサービス
│   │   ├── webrtc/            # WebRTCサービス
│   │   ├── input/             # 入力サービス
│   │   ├── hostd/             # 統合バイナリ
│   │   └── web/               # Web UI
│   │       └── index.html
│   └── frontend/              # C# UI（未実装）
└── web/                       # TanStack Start/ReactベースのWebクライアントPoC
```

## 開発メモ

### 動作確認済み
- ✅ ワークスペースのビルド
- ✅ 単一バイナリでの起動
- ✅ HTTPサーバによるWeb UI配信
- ✅ WebSocket接続の確立
- ✅ SDP Offer/Answerのやり取り（webrtc-rsによる実際のAnswer生成）
- ✅ ICE candidateの処理
- ✅ PeerConnectionの作成と状態監視
- ✅ ダミーフレームの生成と送信
- ✅ SignalingServiceからWebRtcServiceへのメッセージ転送（チャネル配線修正後）
- ✅ **VideoTrackへのフレーム投入（VP8/VP9/H.264 ソフトウェアエンコード）**
  - CaptureServiceのRGBAダミーフレームを encoder クレート経由でVP9/VP8/H.264 にエンコード（初期2ワーカー）
  - WebRTC VideoTrack経由でブラウザに送信（VP9 > VP8 > H.264 の優先順で選択）
  - ブラウザのvideoタグでダミーフレームが表示されることを確認
- ✅ DataChannel開通（クライアント → WebRtcService → InputService でJSONメッセージを受信しログ出力）

### 棚上げ・未実装項目
- ⚠️ 接続ごとのチャンネル管理（グローバルbroadcast依存）
- ⚠️ 実際のウィンドウキャプチャ（Windows GraphicsCapture API）
- ⚠️ Win32 SendInputによる入力注入
- ⚠️ スクリーンショット取得・返却
- ⚠️ ハードウェアエンコード検討（NVENC/AMF/QuickSync）

### 参考資料
- [SPEC.md](SPEC.md): プロダクト仕様
- [desktop/services/README.md](desktop/services/README.md): サービス詳細

## 注意事項

- webrtc-rsで実際のAnswer/ICE生成まで実装済み（VP8/VP9/H.264ソフトウェアエンコード）
- **VideoTrack送信は実装済み**：ダミーフレームをエンコードしブラウザに表示可（CPU負荷は未計測）
- DataChannelは開通しているが、入力注入・スクリーンショット処理は未実装（ログのみ）
- 現状は動作確認用の最小構成。実プレイには実キャプチャ・入力注入・複数接続対応が必須
- 本番運用には接続ごとのチャンネル管理や再接続ハンドリングが必要
- Windows専用機能を前提としており、クロスプラットフォーム対応は未検討

## 実装の詳細

### webrtc-rs統合
- **バージョン**: 0.14（`webrtc-rs`別名で依存）
- **実装内容**:
  - MediaEngine + InterceptorRegistry 初期化、host-only ICE（STUNなし）
  - Offer受信→Answer生成、ICE候補送受信
  - PeerConnection/ICE状態・RTCP (PLI/FIR)・送信統計の監視
  - Track受信ハンドラの設定
  - **VideoTrack送信**:
    - TrackLocalStaticSampleを追加し、接続完了まではフレームをドロップ
    - 要求/ビルド済みコーデックから VP9 > VP8 > H.264 を選択し encoder ワーカーに投入
    - webrtc_media::Sampleとして送出し、PLI/FIRでキーフレーム再送をトリガー
  - **DataChannel**:
    - ブラウザのDataChannelを受信し、JSONを`InputService`へ転送（現状ログのみ）

### 映像エンコード統合
- **H.264 (OpenH264 0.9)**:
  - RGB変換→YUVBuffer→OpenH264エンコード、Annex-Bへ整形（SPS/PPS検出）
- **VP8/VP9 (vpx-rs 0.2.1)**:
  - RGBA→I420変換後にlibvpxでリアルタイムエンコード、初期数フレームを強制キーフレーム
- **共通**:
  - 初期解像度1280x720、ワーカー数は`hostd`で2に設定（ラウンドロビン投入）
  - EncodeResultをtokioチャネルで集約し、Trackへ書き込み

### シグナリングフロー
1. クライアントがWebSocket経由でOfferを送信
2. SignalingServiceがOfferを受信してWebRtcServiceに転送（mpscチャンネル経由）
3. WebRtcServiceがwebrtc-rsを使用してAnswerを生成
4. Answerをbroadcastチャンネル経由でSignalingServiceに送信
5. SignalingServiceがAnswerをクライアントに送信
6. ICE candidateも同様のフローで処理

### 修正履歴

#### 2025年12月: VP8/VP9対応とDataChannel配線
- **実装内容**:
  - `encoder`クレートを追加し、OpenH264 0.9 / vpx-rs 0.2.1 によるVP8/VP9/H.264ソフトウェアエンコードを実装（複数ワーカー）
  - Web UIにコーデック選択UIを追加し、WebRtcService側で優先順選択を実装
  - DataChannelメッセージをInputServiceに中継（現在はログ）
  - hostdでコーデックfeatureを必須化し、起動時にCaptureServiceを自動開始
- **結果**: VP8/VP9/H.264いずれでもダミーフレームを送出でき、DataChannel経由で入力リクエストを受信可能（処理は未実装）

#### 2025年12月: WebRTCメッセージチャネル配線の修正
- **問題**: `hostd`でWebRTCメッセージ用チャネルを二重に作成していたため、SignalingServiceからWebRtcServiceへのメッセージ転送が機能していなかった
- **修正内容**:
  - `hostd/src/main.rs`で重複していたチャネル作成を削除
  - `WebRtcService::new`から返される`webrtc_msg_tx`をそのまま`SignalingService`に渡すように変更
- **結果**: Offer/Answerのやり取りが正常に動作するようになった

