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

### Rust バックエンド (desktop/services/)

```bash
cargo build --release                    # ビルド
cargo run --bin hostd                    # デフォルト設定で実行
cargo run --bin hostd -- --port 8080 --log-level debug  # オプション付きで実行
cargo run --bin hostd -- --mock          # ダミーフレームで実行（テスト用）
cargo check                              # コンパイル確認
cargo test                               # テスト実行
cargo bench --package encoder            # エンコーダのベンチマーク
```

### Web クライアント (client/apps/web/)

```bash
pnpm install          # 依存関係インストール
pnpm dev              # 開発サーバー (ポート 3000)
pnpm build            # プロダクションビルド
pnpm lint             # oxlint でリント
pnpm lint:fix         # リント問題を修正
pnpm fmt              # oxfmt でフォーマット
pnpm dlx shadcn@latest add <component>  # shadcn コンポーネント追加
```

### シグナリングサーバー (signaling/)

```bash
pnpm install          # 依存関係インストール
pnpm run dev          # ローカル開発サーバー (wrangler)
pnpm run deploy       # Cloudflare にデプロイ
```

### タスクランナー

利用可能なタスクはルートの `Taskfile.yml` を参照。

## アーキテクチャ

### 全体構成

```
クライアント ──WebSocket──> シグナリングサーバー ──WebSocket──> hostd
(Web / Android)           (Cloudflare Workers)            (Windows)
        ↑                                                    │
        └──────────── WebRTC (映像・音声・入力) ──────────────┘
```

- **hostd** (Windows): ゲーム画面キャプチャ → エンコード → WebRTC で配信。入力を DataChannel 経由で受信しゲームに反映
- **シグナリングサーバー** (Cloudflare): hostd とクライアント間の WebRTC 接続確立を仲介
- **Web クライアント**: ブラウザ上で WebRTC ストリームを受信・表示し、タッチ/クリック入力を送信
- **Android クライアント**: ネイティブアプリで同等の機能を提供
- **デスクトップ UI** (WinUI 3): hostd の設定・操作を行うデスクトップアプリ

### ディレクトリ構成

```
remoterg/
├── desktop/
│   ├── services/        # Rust バックエンド (Cargo ワークスペース)
│   └── Ui/              # デスクトップ UI (WinUI 3 / C# XAML)
├── client/              # フロントエンド (pnpm モノレポ)
│   ├── apps/web/        #   Web クライアント (TanStack Start)
│   ├── apps/mobile/     #   モバイルクライアント (React Native) ※Kotlin移行中で状況は kotlin-walkthrough.md に保存
│   └── packages/        #   共有パッケージ (core, ui, webrtc)
├── android/             # Android ネイティブ (Kotlin / Jetpack Compose)
├── signaling/           # シグナリングサーバー (Cloudflare Workers)
└── scripts/             # ユーティリティスクリプト
```

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

## 技術スタック

### バックエンド

- tokio 1.40, webrtc-rs 0.14
- Media Foundation H.264 (ハードウェア), OpenH264 (フォールバック)
- tokio-tungstenite (WebSocket クライアント)

### Web フロントエンド

- TanStack Start, React 19, Tailwind CSS 4
- Sentry エラートラッキング
- `any` 型の使用は禁止

### Android

- Kotlin, Jetpack Compose, Hilt, Room
- Ktor (HTTP / WebSocket クライアント)
- WebRTC (libwebrtc)

### シグナリングサーバー

- Cloudflare Workers + Durable Objects
- TypeScript, valibot