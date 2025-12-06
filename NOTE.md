# RemoteRG 実装状況メモ

最終更新: 2025年12月（VideoTrackへのフレーム投入実装完了）

## プロジェクト概要

自宅のWindows PC上で動いているノベルゲームを、スマホ／タブレットから映像付きでリモートプレイできる仕組みを開発中。

詳細仕様は [SPEC.md](SPEC.md) を参照。

## 現在の実装状況

### ✅ 実装完了

#### 1. プロジェクト構造
- Rustワークスペース構成（`desktop/services/`）
- マイクロサービス風のクレート分割：
  - `capture`: キャプチャサービス
  - `signaling`: HTTP/WebSocketシグナリングサーバ
  - `webrtc`: WebRTC接続管理（スケルトン）
  - `input`: 入力処理サービス（スケルトン）
  - `hostd`: 統合バイナリ

#### 2. CaptureService
- **状態**: ダミーフレーム生成機能を実装
- **機能**:
  - グラデーション画像のダミーフレーム生成（RGBA形式）
  - 設定可能な解像度・FPS
  - `tokio::mpsc`チャンネル経由でのフレーム送信
  - 開始/停止/設定変更コマンドの受信
- **テスト**: ユニットテスト実装済み
- **未実装**: Windows GraphicsCapture APIによる実際のウィンドウキャプチャ

#### 3. SignalingService
- **状態**: WebSocketシグナリングサーバ実装済み
- **機能**:
  - HTTPサーバ（Axum）によるWeb UI配信
  - WebSocketエンドポイント (`/signal`)
  - SDP Offer受信とWebRTCサービスへの転送
  - WebRTCサービスからのAnswer/ICE candidate受信とクライアントへの送信
  - ICE candidateの受信とWebRTCサービスへの転送
- **実装詳細**:
  - broadcastチャンネルを使用してWebRTCサービスからの応答を受信
  - 各WebSocket接続はbroadcastチャンネルを購読して応答を受信
- **制限事項**: 
  - 現在はbroadcastチャンネルを使用した簡易実装のため、複数接続時の応答の正確な配信が保証されない
  - 接続ごとのチャンネル管理は未実装（TODO: 複数接続対応）

#### 4. Web UI（クライアント側）
- **状態**: 基本的なUIとWebRTC接続処理を実装
- **機能**:
  - 接続ボタンによるWebSocket接続
  - PeerConnection作成とOffer送信
  - Answer受信とRemoteDescription設定
  - 接続状態の表示とログ出力
  - エラーハンドリングと詳細ログ
- **ファイル**: `desktop/services/web/index.html`

#### 5. WebRtcService
- **状態**: webrtc-rsを使用した実装完了
- **実装済み機能**:
  - webrtc-rs（0.14）を使用したPeerConnectionの作成
  - Offerを受信して実際のAnswerを生成
  - ICE candidateの処理（受信・送信）
  - PeerConnection状態の監視
  - Track受信のハンドラ設定
  - SignalingServiceへの応答送信（broadcastチャンネル経由）
  - **VideoTrackへのフレーム投入（実装完了）**
    - H.264用のTrackLocalStaticSampleを作成してPeerConnectionに追加
    - OpenH264エンコーダーを使用したH.264エンコーディング
    - CaptureServiceのRGBAフレームをYUVに変換してエンコード
    - エンコードされたフレームをWebRTC VideoTrackに送信
- **構造**:
  - CaptureServiceからのフレーム受信チャンネル
  - SignalingServiceとのメッセージパッシング
  - webrtc-rsのAPIを使用したPeerConnection管理
  - OpenH264エンコーダーによるH.264エンコーディング
- **未実装**: 
  - DataChannelの実装
  - 接続ごとのチャンネル管理（現在はbroadcastチャンネルで簡易実装）

#### 6. InputService
- **状態**: スケルトン実装
- **機能**:
  - DataChannelメッセージの受信とログ出力
  - キー入力・マウスホイール・スクリーンショットリクエストのメッセージ定義
- **未実装**: Win32 SendInput APIによる実際の入力注入

#### 7. hostd（統合バイナリ）
- **状態**: 全サービスを統合した単一バイナリ実装済み
- **機能**:
  - CLIオプション（ポート番号、ログレベル）
  - 全サービスの並列実行（tokio::select!）
  - チャンネルによるサービス間通信の設定
  - **CaptureServiceの自動開始**
    - 起動時に自動的にダミーフレーム生成を開始
- **修正履歴**:
  - WebRTCメッセージチャネルの配線ミスを修正（2025年12月）
    - `WebRtcService::new`から返されるチャネルを正しく使用するように変更
    - SignalingServiceからWebRtcServiceへのメッセージ転送が正常に動作するようになった

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
- **原因**: broadcastチャンネルを使用した簡易実装のため、接続ごとの正確な応答配信が保証されない
- **影響**: 複数のクライアントが同時に接続した場合、応答が正しく配信されない可能性がある
- **対応**: 接続ごとのチャンネル管理を実装する必要がある（TODO）

### 2. VideoTrackへのフレーム投入 ✅ 実装完了
- **状態**: 実装完了
- **実装内容**:
  - H.264用のTrackLocalStaticSampleを作成してPeerConnectionに追加
  - OpenH264エンコーダーを使用したH.264エンコーディング
  - CaptureServiceのRGBAフレームをYUVBufferに変換
  - エンコードされたフレームをwebrtc_media::Sampleとして送信

### 3. 実際のウィンドウキャプチャ未実装
- **現状**: ダミーフレームのみ生成
- **必要**: Windows GraphicsCapture APIの統合
- **依存**: Windows SDK、適切なRustバインディング

### 4. DataChannel未実装
- **現状**: PeerConnectionは作成されているが、DataChannelの確立とメッセージ処理が未実装
- **必要**: DataChannelの確立、クライアントからの入力メッセージ受信、InputServiceへの転送
- **依存**: webrtc-rsのDataChannel APIの調査と実装

### 5. 入力注入未実装
- **現状**: ログ出力のみ
- **必要**: Win32 SendInput APIの実装
- **依存**: `windows-rs`または`winapi`クレート

### 6. スクリーンショット機能未実装
- **現状**: リクエスト受信のみ
- **必要**: キャプチャフレームの保存とクライアントへの送信

## 次のステップ（優先順位順）

### 短期（動作確認フェーズ）

1. **接続ごとのチャンネル管理の実装** ⚠️ 棚上げ
   - 現在はbroadcastチャンネルで簡易実装
   - 接続ごとに独立した応答チャンネルを管理する必要がある
   - WebRTCサービスが接続IDとチャンネルのマッピングを管理
   - 複数接続時の正確な応答配信を保証

2. **VideoTrackへのフレーム投入** ✅ 実装完了
   - H.264用のTrackLocalStaticSampleを作成してPeerConnectionに追加
   - OpenH264エンコーダーを使用したH.264エンコーディング
   - CaptureServiceのRGBAフレームをYUVBufferに変換してエンコード
   - エンコードされたフレームをWebRTC VideoTrackに送信

3. **GraphicsCapture APIの統合**
   - Windows GraphicsCapture APIの調査
   - Rustバインディングの選択（`windows-rs`など）
   - HWND指定によるウィンドウキャプチャの実装

### 中期（機能実装フェーズ）

4. **DataChannelの実装** ⚠️ 棚上げ
   - WebRTC DataChannelの確立
   - クライアントからの入力メッセージ受信
   - InputServiceへの転送

5. **Win32 SendInputの実装**
   - キー入力の注入
   - マウス操作の注入
   - ゲーム操作への抽象化

### 長期（品質向上フェーズ）

6. **スクリーンショット機能**
   - フレームの保存（ホスト側）
   - クライアントへの画像送信
   - ダウンロード機能

7. **C# UI（WinUI）の実装**
   - IPCプロトコルの定義
   - UI実装
   - 設定管理

8. **セキュリティ機能**
   - PINコード認証
   - Tailscale統合時のアクセス制御

9. **パフォーマンス最適化**
    - エンコーディング最適化
    - レイテンシ削減
    - メモリ使用量の最適化

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
│         │ DataChannel (未実装)     │
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
- **WebRTC**: ブラウザネイティブAPI
- **UI**: バニラJavaScript + HTML/CSS

### 現在使用中
- **WebRTC**: webrtc-rs 0.14（Rustネイティブ実装）
- **H.264エンコーディング**: openh264 0.4
- **WebRTCメディア**: webrtc-media 0.11
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
│   │   ├── capture/           # キャプチャサービス
│   │   ├── signaling/         # シグナリングサービス
│   │   ├── webrtc/            # WebRTCサービス
│   │   ├── input/             # 入力サービス
│   │   ├── hostd/             # 統合バイナリ
│   │   └── web/               # Web UI
│   │       └── index.html
│   └── frontend/              # C# UI（未実装）
└── web/                       # 将来のWebクライアント拡張（未使用）
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
- ✅ **VideoTrackへのフレーム投入（H.264エンコーディング）**
  - CaptureServiceのRGBAダミーフレームをH.264エンコード
  - WebRTC VideoTrack経由でブラウザに送信
  - ブラウザのvideoタグでダミーフレームが表示されることを確認

### 棚上げ・未実装項目
- ⚠️ 接続ごとのチャンネル管理（現在はbroadcastチャンネルで簡易実装）
- ⚠️ DataChannelの実装
- ⚠️ 実際のウィンドウキャプチャ（Windows GraphicsCapture API）
- ⚠️ Win32 SendInputによる入力注入

### 参考資料
- [SPEC.md](SPEC.md): プロダクト仕様
- [desktop/services/README.md](desktop/services/README.md): サービス詳細

## 注意事項

- webrtc-rsを使用した実際のWebRTC Answer生成まで実装完了
- **VideoTrackへのフレーム投入が実装完了** - ダミーフレームがH.264エンコードされてブラウザに送信される
- 現在の実装は**動作確認可能な最小構成**です
- 実際のゲームプレイには、上記の未実装機能（DataChannel、実際のキャプチャ、入力注入）の追加が必要です
- 複数接続対応は簡易実装のため、本番環境では接続ごとのチャンネル管理が必要です
- Windows固有のAPI統合が必要なため、クロスプラットフォーム対応は現時点では考慮していません

## 実装の詳細

### webrtc-rs統合
- **バージョン**: 0.14
- **使用方法**: `webrtc-rs`という別名で依存関係に追加（ローカルの`webrtc`クレートと名前衝突を回避）
- **実装内容**:
  - MediaEngineとInterceptorRegistryの初期化
  - PeerConnectionの作成と設定（ICE設定はホストオンリー）
  - Offerの受信とAnswerの生成
  - ICE candidateの処理（受信・送信）
  - PeerConnection状態の監視
  - Track受信のハンドラ設定
  - **VideoTrack送信の実装**:
    - H.264用のTrackLocalStaticSampleを作成
    - PeerConnectionにTrackを追加（add_track）
    - OpenH264エンコーダーでH.264エンコーディング
    - webrtc_media::Sampleとしてフレームを送信

### H.264エンコーディング統合
- **エンコーダー**: OpenH264 0.4
- **実装内容**:
  - EncoderConfig::new()でエンコーダーを初期化
  - CaptureServiceのRGBAフレームをYUVBuffer::with_rgb()でYUVに変換
  - Encoder::encode()でH.264エンコード
  - エンコードされたビットストリームをwebrtc_media::Sampleに変換
  - TrackLocalStaticSample::write_sample()で送信
- **解像度**: デフォルト1280x720、フレームごとに動的変更対応

### シグナリングフロー
1. クライアントがWebSocket経由でOfferを送信
2. SignalingServiceがOfferを受信してWebRtcServiceに転送（mpscチャンネル経由）
3. WebRtcServiceがwebrtc-rsを使用してAnswerを生成
4. Answerをbroadcastチャンネル経由でSignalingServiceに送信
5. SignalingServiceがAnswerをクライアントに送信
6. ICE candidateも同様のフローで処理

### 修正履歴

#### 2025年12月: WebRTCメッセージチャネル配線の修正
- **問題**: `hostd`でWebRTCメッセージ用チャネルを二重に作成していたため、SignalingServiceからWebRtcServiceへのメッセージ転送が機能していなかった
- **修正内容**:
  - `hostd/src/main.rs`で重複していたチャネル作成を削除
  - `WebRtcService::new`から返される`webrtc_msg_tx`をそのまま`SignalingService`に渡すように変更
- **結果**: Offer/Answerのやり取りが正常に動作するようになった

#### 2025年12月: VideoTrackへのフレーム投入実装
- **実装内容**:
  - `index.html`にvideo recvonly transceiverを追加
  - `WebRtcService`にH.264用のTrackLocalStaticSampleを追加
  - OpenH264エンコーダーを使用したH.264エンコーディング実装
  - CaptureServiceのRGBAフレームをYUVBufferに変換してエンコード
  - エンコードされたフレームをwebrtc_media::Sampleとして送信
  - `hostd`でCaptureServiceを起動時に自動開始
- **依存関係追加**:
  - `openh264 = "0.4"` - H.264エンコーディング
  - `webrtc-media = "0.11"` - WebRTCメディアサンプル
  - `bytes = "1.0"` - バイトデータ処理
- **結果**: ブラウザのvideoタグにダミーフレームがH.264エンコードされて表示されるようになった

