# RemoteRG Host Daemon

ノベルゲームをリモートプレイするためのホスト側デーモンです。

## ビルド

```bash
cargo build --release
```

## 実行

```bash
cargo run --bin hostd
```

オプション:
- `-p, --port <PORT>`: HTTP/WebSocketサーバのポート番号（デフォルト: 8080）
- `-l, --log-level <LEVEL>`: ログレベル（trace, debug, info, warn, error、デフォルト: info）

例:
```bash
cargo run --bin hostd -- --port 9000 --log-level debug
```

## 動作確認手順

1. ホストデーモンを起動:
   ```bash
   cargo run --bin hostd
   ```

2. ブラウザで `http://localhost:8080` にアクセス

3. 「接続」ボタンをクリック

4. WebSocket接続が確立され、ダミーのSDP Answerが返されることを確認

## アーキテクチャ

### サービス構成

- **CaptureService**: ウィンドウキャプチャ（現在はダミーフレーム生成）
- **WebRtcService**: WebRTC接続管理（現在はスケルトン）
- **SignalingService**: HTTP/WebSocketサーバによるシグナリング
- **InputService**: クライアントからの入力処理（現在はログ出力のみ）

### メッセージフロー

```
クライアント (ブラウザ)
    ↓ WebSocket
SignalingService
    ↓ SDP Offer/Answer
WebRtcService
    ↓ フレーム
CaptureService
    ↓ DataChannel
InputService
```

## 開発状況

現在は最小限の実装が完了しています:

- ✅ ワークスペースとクレート構造
- ✅ ダミーフレーム生成（CaptureService）
- ✅ WebSocketシグナリングサーバ（SignalingService）
- ✅ WebRTCサービスのスケルトン
- ✅ 単一バイナリでの起動

次のステップ:
- [ ] GraphicsCapture APIを使った実際のウィンドウキャプチャ
- [ ] libwebrtcバインディングの統合
- [ ] Win32 SendInputによる入力注入
- [ ] スクリーンショット機能

