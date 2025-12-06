1. プロダクト概要

自宅の Windows PC 上で動いているノベルゲームを、 スマホ／タブレットから 映像付きでリモートプレイできる仕組み。
一般的なリモートデスクトップではなく、特定ウィンドウ限定のストリーミング、ノベルゲームに必要な操作だけに絞ったコントローラUI、スクリーンショットのホスト＆クライアント両保存を特徴とする。

2. 想定ユースケース

同じ家の別の場所（ベッド・ソファなど）で、デスクトップPC上のノベルゲームをスマホ／タブレットから遊ぶ。
旅行先など外出先から、自宅のPC上のノベルゲームをプレイする。
ノベルゲーム特有の演出・ムービー・エフェクトも WebRTC による映像ストリーミングで滑らかに再生。

3. 全体構成（ざっくり）

ホスト（Windows）
UI：C#（WinUI）
コアロジック：Rust（GraphicsCapture + libwebrtc）

クライアント（スマホ／タブレット）
Webブラウザで動く Web アプリ（PWA 想定）
WebRTC で映像・音声を再生
DataChannel 経由でコントローラ入力送信

接続形態
同一 LAN：IP 直打ち or mDNS
外出先：Tailscale 経由で同一ネットワーク的に扱う


4. ホスト側アプリ仕様（Windows）

4-1. C# UI（フロント）

機能
起動・終了管理
対象ノベルゲームウィンドウの選択（HWND 選択）
映像設定：
解像度（例：1280x720, 1600x900）
FPS（30/60）
ビットレート
スクリーンショット保存ディレクトリ設定
ステータス表示（接続中、視聴クライアント数、現在のFPS等）

Rustコアとの連携
JSONベースのIPC（ローカルTCP or Named Pipe）で以下に代表されるコマンドを送信
- start_session
- stop_session
- update_config

4-2. Rust コア（バックエンド）

役割
- 指定された HWND のウィンドウを GraphicsCapture でキャプチャ
- libwebrtc を使って H.264 映像＋Opus音声を WebRTC で配信
- DataChannel でクライアントからの入力を受信し、SendInput でゲームへ注入
- スクリーンショットを撮影し、ホスト保存＋クライアントへ送信
- シグナリング用の HTTP + WebSocket サーバを内蔵し、Web UI もここから配信

5. クライアント側仕様（スマホ／タブレット）

形態：ブラウザベース Web アプリ（PWA 前提）
機能：
- ホストの Web UI にアクセスし、WebSocket 経由でシグナリング
- WebRTC の PeerConnection を生成し、映像・音声ストリームを <video> に表示
- DataChannel を用いて以下のような操作を送信：
  - 「次へ（Enterキー相当）」
  - オートモード、スキップ、既読スキップ等のゲーム操作に対応した抽象アクション
  - マウススクロール、クリックなど
スクリーンショットリクエスト送信と、受信した画像の保存（ダウンロード or アプリ内ギャラリー）


6. 通信・プロトコル

6-1. シグナリング

Rust コアに内蔵された HTTP + WebSocket サーバを使用。

手順：
  - クライアントがホストの URL にアクセス（LAN: IP/mDNS, 外出先: Tailscale）
  - WebSocket (/signal) で接続
  - クライアントで createOffer → SDP を WebSocket で送信
  - Rust側で libwebrtc に渡し、createAnswer → SDP を返却
  - ICE candidate を双方向に WebSocket でやり取り

6-2. DataChannel メッセージ例

入力操作
  - { "type": "key", "key": "ENTER", "down": true }
  - { "type": "mouse_wheel", "delta": -120 }
スクリーンショット
  - { "type": "screenshot_request" }

7. Rust 内部アーキテクチャ（マイクロサービス風）

プロセスは 1 つだが、中を「役割ごとのサービス」として分割し、
非同期タスク＋メッセージパッシングで連携する。


主なサービス

1. ControlService
C# UI からの IPC を受けて、セッション開始／停止、設定変更を指示

2. CaptureService
HWND を受け取り、GraphicsCapture でフレーム取得
フレームイベントを他サービス（WebRTC）に送信

3. WebRtcService
libwebrtc をラップ
CaptureService からのフレームを WebRTC VideoTrack に投入
DataChannel メッセージを InputService に転送

4. SignalingService
HTTP/WS サーバとしてクライアントとシグナリング、Web UI 配信

5. InputService
DataChannel からの抽象的な入力コマンドを受け取り、Win32 SendInput で実際のキー・マウス操作に変換

各サービス間は tokio::mpsc などのチャンネルで疎結合に接続し、
マイクロサービスライクな構造を保ちつつ、最終的には単一バイナリで配布。

8. 接続パターン

LAN 内
PCとスマホが同じWi-Fi
接続方法：
IP直打ち（例：http://192.168.0.10:8080）
余裕があれば mDNS でホスト名から解決（将来的にネイティブラッパーで対応）

外出先
ホストPCとスマホ両方に Tailscale を導入
Tailnet 内のアドレス（例：https://hostname.tailnet-name.ts.net:8080）でアクセス
一般的なNAT越え問題は Tailscale 側に任せる方針

9. その他・非機能要件（ラフ）
ノベルゲームのムービーや演出再生が快適なレベルの遅延・画質を目標
目標：30fps / 720p を安定して配信（LANでは60fpsも検討）

セキュリティ
LAN内：簡易なPINコード認証
Tailscale使用時：Tailnetレベルのアクセス制御＋アプリ側PINで二重化

将来的に：
ゲームごとのプリセット（「Enterが次へ」「Ctrlが既読スキップ」など）
複数クライアント接続（観戦モード）も拡張可能な構成にしておく
