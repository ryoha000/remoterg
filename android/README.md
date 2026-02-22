# RemoteRG Android クライアント

Windows 上のビジュアルノベルのゲーム画面を WebRTC でストリーミング受信し、タッチ入力を送信するネイティブ Android アプリ。

## 技術スタック

| カテゴリ | ライブラリ |
|---|---|
| UI | Jetpack Compose / Material 3 |
| DI | Hilt |
| ローカル DB | Room |
| HTTP / WebSocket | Ktor Client (OkHttp) |
| WebRTC | libwebrtc (`io.github.nichenqin:webrtc-android`) |
| 画像読み込み | Coil |
| シリアライズ | kotlinx.serialization |
| テスト | JUnit 4 / MockK / Turbine / kotlinx-coroutines-test |

## アーキテクチャ

```
┌─────────────────────────────────────────────────┐
│                    ui/screens/                   │
│  ConnectScreen  ViewerScreen  GalleryScreen ...  │
│         ↕ collect StateFlow                      │
│                  ui/viewmodel/                   │
│      ViewerViewModel    GalleryViewModel         │
│         ↕ inject (Hilt)                          │
├──────────┬──────────┬────────────────────────────┤
│ webrtc/  │ domain/  │ data/                      │
│ IWebRtc  │ Screens  │ Repository  Room  Entity   │
│ Manager  │ hotProc  │                            │
│ ISignal  │          │                            │
│ ingClnt  │          │                            │
└──────────┴──────────┴────────────────────────────┘
```

### レイヤーと責務

| レイヤー | パッケージ | 責務 |
|---|---|---|
| **Screen** | `ui/screens/` | Composable UI。ViewModel の StateFlow を collect して表示 |
| **State Holder** | `ui/screens/ViewerStateHolders.kt` | Overlay・ZoomPan・PiP などの UI 状態管理 |
| **ViewModel** | `ui/viewmodel/` | ビジネスロジックと UI ステートの橋渡し |
| **Domain** | `domain/` | ユースケース (スクリーンショット処理など) |
| **WebRTC** | `webrtc/` | WebRTC 接続管理とシグナリング |
| **Data** | `data/` | Room DB、Repository、MediaStore アクセス |
| **DI** | `di/` | Hilt Module 定義 |
| **Util** | `ui/util/` | 純関数ユーティリティ (レイアウト計算など) |

### ディレクトリ構造

```
app/src/main/java/moe/ryoha/remoterg/
├── MainActivity.kt              # NavHost とナビゲーション定義
├── RemotergApplication.kt       # @HiltAndroidApp
├── di/
│   ├── WebRtcModule.kt          # IWebRtcManager, ISignalingClient の提供
│   ├── DatabaseModule.kt        # Room DB の提供
│   ├── RepositoryModule.kt      # Repository の提供
│   └── CoilModule.kt            # Coil ImageLoader の提供
├── ui/
│   ├── screens/
│   │   ├── ConnectScreen.kt     # 接続画面
│   │   ├── ViewerScreen.kt      # ゲーム映像表示画面
│   │   ├── ViewerStateHolders.kt # Viewer の UI 状態管理
│   │   ├── ScreenshotFlash.kt   # スクリーンショットアニメーション
│   │   ├── GalleryScreen.kt     # ギャラリー一覧
│   │   ├── GalleryDetailScreen.kt # ギャラリー詳細
│   │   └── SearchPanel.kt       # ギャラリー検索パネル
│   ├── viewmodel/
│   │   ├── ViewerViewModel.kt   # 接続フロー管理
│   │   └── GalleryViewModel.kt  # ギャラリーデータ管理
│   ├── theme/                   # Material 3 テーマ定義
│   └── util/
│       └── JustifiedLayoutCalculator.kt  # Justified レイアウト計算
├── domain/
│   └── ScreenshotProcessor.kt   # DataChannel 経由のスクリーンショット処理
├── webrtc/
│   ├── IWebRtcManager.kt        # WebRTC 管理インターフェース
│   ├── WebRtcManager.kt         # WebRTC 管理実装
│   ├── WebRtcVideoRenderer.kt   # SurfaceViewRenderer Composable
│   ├── SdpUtils.kt              # SDP コーデック優先制御
│   ├── DataChannelMessage.kt    # DataChannel メッセージ型
│   ├── SimpleSdpObserver.kt     # SDP Observer 基底クラス
│   └── signaling/
│       ├── ISignalingClient.kt  # シグナリングインターフェース
│       ├── SignalingClient.kt   # WebSocket シグナリング実装
│       └── SignalingMessage.kt  # シグナリングメッセージ型
└── data/
    ├── local/
    │   ├── AppDatabase.kt       # Room Database 定義
    │   ├── dao/                 # DAO インターフェース
    │   └── entity/              # Room Entity
    ├── model/                   # ドメインモデル
    └── repository/
        └── ScreenshotRepository.kt  # MediaStore + Room のスクリーンショット管理
```

## よく使うコマンド

```bash
# ビルド
./gradlew assembleDebug

# ユニットテスト実行
./gradlew testDebugUnitTest

# 特定のテストクラスのみ実行
./gradlew testDebugUnitTest --tests "moe.ryoha.remoterg.ui.util.JustifiedLayoutCalculatorTest"

# リリースビルド
./gradlew assembleRelease

# デバイスにインストール＆起動
./gradlew installDebug
adb shell am start -n moe.ryoha.remoterg/.MainActivity

# lint
./gradlew lint
```

## コード追加ガイドライン

### 全般

- **言語**: Kotlin。`any` 型やプラットフォーム型の使用は避ける
- **コメント**: 日本語で記述する
- **DI**: 新しいクラスは Hilt の `@Inject` / `@Module` で管理する
- **テスト**: ビジネスロジックには必ずユニットテストを書く

### 新しい画面を追加する場合

1. `ui/screens/` に `XxxScreen.kt` Composable を作成
2. 必要に応じて `ui/viewmodel/` に `XxxViewModel.kt` を作成（`@HiltViewModel`）
3. `MainActivity.kt` の `NavHost` にルートを追加
4. ViewModel が必要な依存は `di/` のモジュールで提供

### WebRTC / シグナリング関連の変更

- `IWebRtcManager` / `ISignalingClient` **インターフェースを先に更新** し、実装を追従させる
- コールバックではなく **SharedFlow / StateFlow** でイベントを公開する
- ViewModel はインターフェースにのみ依存させ、テスト時は Fake に差し替え可能にする

### データ層の変更

- Room のスキーマ変更時は **マイグレーション** を忘れない（`AppDatabase.kt`）
- MediaStore は Source of Truth。DB はメタデータ（windowTitle, processName 等）の補助

### UI コンポーネント

- べた書きせず **State Holder パターン** を使って状態管理を分離（例: `ViewerStateHolders.kt`）
- 頻繁に変わる状態（FPS, ビットレート）と不変の状態（コーデック, 解像度）は **別の Composable** に分ける（リコンポジション最適化）
- テーマは `ui/theme/` で管理。ダークモードのみ（zinc カラーパレット）

### テスト

- ユニットテストは `app/src/test/` 配下にプロダクションコードと同じパッケージ構造で配置
- Android フレームワーク依存は `mockk(relaxed = true)` でモック化
- `build.gradle.kts` の `testOptions.unitTests.isReturnDefaultValues = true` で `android.util.Log` 等はデフォルト値を返す
- WebRTC / シグナリングのテストは `IWebRtcManager` / `ISignalingClient` の Fake 実装を使用
