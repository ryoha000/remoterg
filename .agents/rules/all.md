---
trigger: always_on
---

## プロジェクト概要

RemoteRG は、Windows上のビジュアルノベルのゲーム画面を WebRTC でスマートフォン/タブレットにストリーミングし、DataChannel で入力を同期するリモートプレイアプリケーション。

- **バックエンド**: Rust 製ホストデーモン (`hostd`) — 画面キャプチャ、エンコード、WebRTC、入力処理を管理
- **Web クライアント**: TanStack Start / React（`client/apps/web/`）
- **Android クライアント**: Kotlin / Jetpack Compose ネイティブアプリ（`android/`）
- **シグナリングサーバー**: Cloudflare Workers + Durable Objects（`signaling/`）

## ビルド & 開発コマンド

### タスクランナー

利用可能なタスクはルートの `Taskfile.yml` を参照。

## 開発ルール

### Rust

- Rust コード修正後、完了前に `cargo check` でコンパイル確認すること
- サービス間の共有型は `core-types` クレートに追加すること
- サービスは他のサービスに直接依存してはならない — `core-types` のみに依存
- エンコーダのファクトリ注入は hostd で行う (feature flags: h264)

### Web

- Sentry エラートラッキングは `src/router.tsx` で設定済み
- サーバー関数は `Sentry.startSpan()` でラップしてインスツルメンテーションすること
- shadcn コンポーネント追加: `pnpm dlx shadcn@latest add <name>`

### Android

- Kotlin / Jetpack Compose で実装
- DI は Hilt を使用
- ローカルデータは Room を使用

### Python

- コードは scripts ディレクトリに配置する
- uv を用いて実行する(python を直で呼び出したり pip install などは禁止)
  - 実行時はカレントディレクトリが scripts ディレクトリとなるようにする
  - 実行時は `$env:PYTHONIOENCODING="utf-8"; uv run python search_titles.py "流星ワールドアクター"` のように `$env:PYTHONIOENCODING="utf-8";` をつける

## その他のルール
設計ドキュメントをあらかじめ示されたうえでコードを編集する際は、必ずドキュメントを先に更新し、その更新内容についてユーザーの確認を経てからコードの編集を行うこと