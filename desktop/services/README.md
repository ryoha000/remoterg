# RemoteRG Host Daemon (`hostd`)

Windows上のノベルゲームをストリーミングし、WebRTC経由でリモートプレイ機能を提供するホスト側デーモンです。

## 他プロダクトからの起動方法

別アプリケーション（ランチャーアプリやプレイ記録ツールなど）から `hostd` を起動する場合、対象となるゲームのウィンドウハンドル（HWND）や、連携用のセッションIDなどを引数として指定してプロセスを起動します。

### 起動コマンド例

```bash
# 基本的な起動（キャプチャ対象のHWNDと一意のセッションIDを指定）
hostd.exe --hwnd <対象ウィンドウのHWND> --session-id <一意のセッションID>

# カスタムシグナリングサーバーを利用する場合
hostd.exe --hwnd <HWND> --session-id <SESSION_ID> --cloudflare-url wss://YOUR_WORKER_URL/api/signal

# デバッグ用途（ログ出力の詳細化やモックの利用）
hostd.exe --hwnd <HWND> --session-id <SESSION_ID> --log-level debug --mock
```

### 主要なオプション

連携時によく利用される実行時オプションは以下の通りです。その他の詳細なオプション（スクリーンショットの保存先やAI関連パスなど）は、`hostd.exe --help` で確認可能です。

- `--hwnd <HWND>`: キャプチャ対象となるウィンドウのハンドル（整数値）。未指定の場合は0扱いとなります。環境変数 `REMOTERG_HWND` にも対応しています。
- `--session-id <SESSION_ID>`: Web/Androidクライアントから接続するための合言葉となるセッションID。 [デフォルト: `fixed`]
- `--cloudflare-url <CLOUDFLARE_URL>`: シグナリングサーバーの WebSocket URL。 [デフォルト: `ws://localhost:8787/api/signal`]
- `-l, --log-level <LOG_LEVEL>`: ログの出力レベル (`trace`, `debug`, `info`, `warn`, `error`)。 [デフォルト: `info`]
- `--mock`: 実際のウィンドウや音声のキャプチャの代わりにモック実装を使用します。キャプチャ対象がない環境での動作検証などに使用します。
- `--llama-server-path <PATH>`: AI機能で利用する `llama-server.exe` へのパス。
- `--character-identifier-models-dir <PATH>`: AIによるキャラクター識別用モデル群が格納されているディレクトリ。
