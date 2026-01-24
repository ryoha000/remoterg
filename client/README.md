# RemoteRG Client Monorepo

RemoteRGのフロントエンドモノレポです。`pnpm workspaces`を使用して管理されています。

## ディレクトリ構成

### apps/
アプリケーションのエントリーポイントを含みます。

- **web**: TanStack Startを使用したWebアプリケーション
- **mobile**: React Nativeを使用したモバイルアプリケーション（実装予定）

### packages/
アプリケーション間で共有されるライブラリコードです。

- **core**: プラットフォーム非依存のビジネスロジック、状態管理、Hooks
- **webrtc**: WebRTC接続処理の抽象化レイヤー
- **ui**: アプリケーション間で共通のUIコンポーネント

## 開発フロー

### セットアップ

ルートディレクトリ（`client/`）で依存関係をインストールします。

```bash
pnpm install
```

### Webアプリの起動

`Taskfile.yml`を経由して実行するか、ディレクトリに移動して直接実行します。

```bash
# プロジェクトルートから
task web

# または
cd apps/web
pnpm dev
```

### パッケージの追加

ワークスペース内のパッケージに依存関係を追加する場合は、`--filter`を使用します。

```bash
# apps/web に @remoterg/ui を追加する場合
pnpm add @remoterg/ui --filter web --workspace
```
