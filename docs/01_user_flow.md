# User Flow Design (White/Black Simple UI)

## コンセプト

- **Focus on Gameplay**: ゲームプレイを阻害しない、没入感を最大化するUI。
- **Monochrome Minimal**: 白と黒を基調としたシンプルで洗練されたトーン。
- **Premium Feel**: 簡素なだけでなく、動きやタイポグラフィで上質さを演出する。

## 1. Landing (接続待機・開始)

ユーザーが最初にアクセスする画面。

- **UI要素**:
  - アプリタイトル/ロゴ (中央配置、ミニマル)
  - ステータスインジケーター (サーバー接続状況)
  - **Connect Button**: 画面中央または下部に配置する主要アクション。
  - (必要に応じて) Host ID / PIN 入力フィールド
- **デザイン**:
  - 背景: メイン背景色 (#09090b) で統一し、暗所での目の負担を軽減。
  - 要素は中心に集約し、十分なホワイトスペース(ネガティブスペース)を確保する。

## 2. Connection Phase (接続処理)

シグナリングおよびWebRTC接続確立中の遷移画面。

- **Action**:
  - ユーザーが接続を開始。
- **UI要素**:
  - 繊細なローディングアニメーション (例: 細いラインのパルス、あるいはブラー処理されたロゴ)
  - ステータス表示 ("Establishing Connection...", "Secure Handshake...")
- **UX**:
  - 画面遷移はスムーズなクロスフェードで行う。
  - 失敗時は、技術的なエラーログではなく「ホストが見つかりません」等のユーザーフレンドリーなメッセージを表示。

## 3. Gameplay (プレイ画面 / Main View)

接続確立後のメイン画面。コンテンツ(ゲーム画面)が最優先。

- **Default State**:
  - **Video Canvas**: ブラウザビューポート全体を使用。黒帯(レターボックス)が出る場合は黒で統一。
  - **UI Elements**: 基本的に**非表示**。
- **Interaction (Hover / Tap)**:
  - マウス移動または画面タップでコントロールオーバーレイがフェードイン。
  - **Floating Action Button (FAB) or Mini Docker**:
    - 画面隅に最小限のメニューアイコンを配置。
- **Overlay UI (Active時)**:
  - **Top Bar**:
    - Network Status (RTT, Bitrateの簡易表示 - 3本線アイコンなど)
    - Settings Icon
  - **Virtual Controls (Mobile)**:
    - 必要に応じてオンスクリーンコントローラーを表示 (不透明度調整機能付き)。

## 4. In-Game Menu (Overlay)

プレイ環境を調整するための設定画面。

- **Access**: OverlayのSettingsアイコンから。
- **UI要素**:
  - **Stream Settings**: 画質 (Quality), 解像度, バンド幅制限
  - **Input**: マウスモード (Relative/Absolute), 感度
  - **Audio**: ボリューム, ミュート
  - **Debug**: 詳細統計 (Stats for Nerds) のトグル
  - **Action**: Disconnect (切断)
- **デザイン**:
  - グラスモーフィズム (背景のゲーム画面をブラー処理 + 半透明レイヤー)。
  - フォントはサンセリフ体 (Inter/Roboto) で可読性を確保。

## 5. Disconnected (終了・切断)

セッション終了時。

- **UI要素**:
  - "Session Ended" メッセージ
  - **Reconnect Button**: 目立つ位置に配置。
  - **Back to Home Button**
- **デザイン**:
  - Landing画面と同様の静謐な雰囲気に戻る。

## デザインガイドライン (Draft)

- **Colors**:
  - Main Background: `#09090b` (Zinc-950)
  - Surface: `#18181b` (Zinc-900)
  - Text Main: `#fafafa` (Zinc-50)
  - Text Muted: `#a1a1aa` (Zinc-400)
  - Border: `#27272a` (Zinc-800)
- **Typography**:
  - Geist, "Geist Fallback"
  - タイトルや重要な数字には太字を使用しコントラストを付ける。
- **Motion**:
  - フェードイン・アウト: `duration-200 ease-in-out`
  - スケールエフェクト: ボタンホバー時に微細な拡大。
