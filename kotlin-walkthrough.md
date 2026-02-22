# 🚀 Android (Kotlin) ネイティブ移行 Walkthrough

React Native (`client/apps/mobile`) から Jetpack Compose ベースの純粋な Android プロジェクト (`android`) への基盤移行が完了しました！

## ✨ 達成したこと (What we accomplished)

1. **Android 基盤ディレクトリへの再編**
   - リポジトリのルート直下に `android` ディレクトリを作成し、`moe.ryoha.remoterg` としてプロジェクトの Scaffold を構築しました。
   - `libs.versions.toml` (Version Catalogs) を活用した最新の Gradle 構成で依存モジュールを一元管理するようにしました。

2. **Drizzle から Room へのローカルDB移行**
   - React Native の SQLite 側で管理していたスキーマ分析 (`analysis_results`), お気に入り (`screenshot_favorites`), プロセスマップ (`screenshot_map`) のテーブル構造を Room 用の `Entity` および `DAO` インターフェース、`AppDatabase` として再定義しました。

3. **WebRTC と Signaling (WebSocket) の再構築**
   - **Ktor WebSockets**: `SignalingClient` を介して、ホスト側のサーバーと直接送受信を行う仕組みを作成し、状態（`Offer/Answer`, `ICE Candidate`）を Jsonパースできるよう構築しました。
   - **libwebrtc・VideoTrack管理**: ネイティブ API（`PeerConnection` 等）を直接扱うための `WebRtcManager` を作成し、`SurfaceViewRenderer` を Compose から呼び出せる `WebRtcVideoRenderer` ラッパーを作成しました。（これにより React Native 経由での描画遅延や PiP サイズ崩れなどを根本から解決できる基盤となります）

4. **Jetpack Compose + MVVM アーキテクチャ構成**
   - **Hilt (DI)** による依存性注入を設定し、MainActivity に `@AndroidEntryPoint` を付加。ViewModel への依存性供給を繋ぎました。
   - **Compose Navigation**: `ConnectScreen` (接続画面) と `ViewerScreen` (表示画面) のルーティングを定義し、UI 状態と WebRTC セッションが連動する ViewModel (`ViewerViewModel`) を構築・接続しました。

5. **シグナリングプロトコルの修正と WebRTC 映像表示の実装**
   - **シグナリングプロトコル準拠**: `SignalingMessage.kt` の型定義をシグナリングサーバーのプロトコルに合わせて修正。`IdentifyMessage` を削除し（URL クエリパラメータ方式）、`@SerialName` で snake_case（`sdp_mid`, `sdp_mline_index`, `ice_candidate`）に対応。`targetId` を全メッセージから除去し、`OfferMessage` に `codec: "h264"` を付与。
   - **SignalingClient の改修**: `connect(url)` に変更（`clientId` 引数・`identify` 送信を削除）。`sendOffer`/`sendAnswer`/`sendIceCandidate` から `targetId` を除去。
   - **WebRTC 接続フローの実装**: RN の `useViewerConnection.ts` と同等のフローを `WebRtcManager.setupConnection()` として実装。recvonly transceiver 追加 → DataChannel 作成 → Offer 生成の順序を明示的に制御。`onRenegotiationNeeded` での自動 Offer 生成を削除し、`onTrack` ハンドラを追加。
   - **ViewerViewModel の接続フロー統合**: Hilt 経由で `Application` context を注入し、`connectToHost()` で `init` → `createPeerConnection` → `connect` → `setupConnection` を順番に実行。
   - **WebRtcVideoRenderer の改善**: `SurfaceViewRenderer` の参照を保持し、`DisposableEffect` で `removeSink` を呼び出してメモリリークを防止。
   - **ViewerScreen のバグ修正**: プレースホルダの `alignment()` 関数を標準の `Modifier.align()` に置換。

6. **接続画面デザインの刷新と codec 選択**
   - **Card ベースの UI**: RN の `ConnectForm.tsx` と同じ構成（タイトル / サブタイトル / Session ID 入力 / Connect + View Gallery ボタン）で `ConnectScreen` を刷新。
   - **codec ドロップダウン**: `h264` / `vp8` / `vp9` / `av1` から選択可能な `ExposedDropdownMenuBox` を追加。選択された codec は `OfferMessage` に動的に反映される。
   - **codec パラメータの伝搬**: `ConnectScreen` → `MainActivity` (ナビゲーション) → `ViewerScreen` → `ViewerViewModel` → `SignalingClient.sendOffer(sdp, codec)` の全経路で codec を受け渡し。

7. **Viewer オーバーレイのデザイン移植**
   - **トップバー**: RN の `ViewerOverlay.tsx` と同等のデザインを `AnimatedVisibility` で実装。戻るボタン（`ArrowBack`）、ステータスバッジ（green/yellow インジケーター + テキスト）、loss バッジ、右側アクションボタン群（デバッグ / ギャラリー / カメラ / 設定）。
   - **デバッグパネル**: FPS / Bitrate / Loss / Session をモノスペースフォントで表示するパネル（現時点はモック値）。
   - **設定パネル**: Audio ボリュームスライダー（モック）+ Disconnect ボタン（赤）。RN の `zinc-900/95` 背景 + `zinc-800` ボーダーを再現。
   - **オーバーレイ表示切替**: タップで表示/非表示を切り替え、4秒後に自動非表示。`AnimatedVisibility(fadeIn/fadeOut)` でアニメーション。
   - **Material Icons Extended**: `androidx.compose.material:material-icons-extended` を依存に追加し、`BugReport` / `CameraAlt` / `CellTower` / `Image` / `Settings` 等のアイコンを使用。

8. **映像表示の object-fit: contain 実装**
   - **問題**: Compose の `fillMaxSize()` を直接適用すると `EXACTLY` 制約が渡され、`SurfaceViewRenderer` 内部の計測ロジックが正常に機能せず `cover` (クロップ) の挙動になってしまう。
   - **解決策**: `AndroidView` の modifier に `wrapContentSize(Alignment.Center)` を追加し、Compose から `AT_MOST` 制約（最大サイズは保持）を渡すよう修正。これにより `SurfaceViewRenderer` が自身のアスペクト比に合わせて正しくリサイズされ、画面中央に配置されることで完全な `contain` (レターボックス) 挙動を実現。

9. **スクリーンショットとギャラリー機能の実装**
   - **DataChannel メッセージ処理**: `WebRtcManager` でバイナリとテキストの送受信をサポートし、`ScreenshotProcessor` で非同期にチャンクを順序通り結合するロジックを実装。
   - **MediaStore 保存**: 組み立てた画像を Android の `MediaStore` に直接保存し、ローカル DB (Room) へメタデータ (`ScreenshotMapEntity`) と共に登録。
   - **Gallery UI**: `LazyVerticalGrid` と `coil-compose` を用いてギャラリー画面を構築。
   - **Repository パターンの導入**: Room DAO と MediaStore 操作を隠蔽する `ScreenshotRepository` を作成し、ViewModel に注入。

10. **ギャラリー詳細画面のアニメーション・ジェスチャー移植**
    - **InfoPanel 連動の画像縮小アニメーション**: RN版 `ScreenshotPage.tsx` の `infoOpenAnim` に相当する `Animatable<Float>` を導入し、InfoPanel 表示時に画像エリアの幅を `screenWidth → screenWidth * 0.65` にアニメーション縮小する仕様を再現。
    - **上下スワイプで一覧に戻るジェスチャー**: `detectVerticalDragGestures` で `offsetY` を追跡し、ドラッグ中のスケール縮小 (0.8) と背景透明度連動を実装。閾値 (100px) 超過でナビゲーションバック、未満でスプリングアニメーションにより復帰。
    - **Header/ActionBar の右端調整**: `fillMaxWidth(fraction = imageAreaFraction)` で InfoPanel 表示時にヘッダーとアクションバーが InfoPanel に被らないよう制御。
    - **背景透明度のドラッグ連動**: RN版 `backdropStyle` の `opacity: anim.value` に相当する `bgAlpha` を導入し、ドラッグ量に応じて背景が透過する仕様を再現。

11. **スクリーンショット フラッシュ + サムネイル縮小アニメーション**
    - RN版 `ScreenshotFlash.tsx` に相当する `ScreenshotFlash.kt` を実装。撮影時の白フラッシュエフェクト、撮影結果画像の表示 → 左下へ縮小移動 → フェードアウトのアニメーションシーケンスを `Animatable` で再現。
    - `ScreenshotImageLayer` / `FlashOverlay` を独立 Composable に分割し、アニメーション値の読み取りが親に波及しないようリコンポジションスコープを最適化。

12. **PiP (Picture-in-Picture) のネイティブ実装**
    - `ViewerScreen.kt` にてネイティブ Android API (`PictureInPictureParams.Builder`, `setPictureInPictureParams`) を直接呼び出し。
    - `setAutoEnterEnabled(true)` による自動 PiP（Android 12+）を実装。アプリ離脱時に自動的に PiP モードへ遷移。
    - `Rational(width, height)` で映像トラックのアスペクト比を動的に計算して PiP ウィンドウに設定。
    - `PictureInPictureModeChangedInfo` リスナーで PiP 状態を監視し、PiP 中はオーバーレイ・フラッシュを非表示にする UI 制御を実装。
    - クリーンアップ時に `setAutoEnterEnabled(false)` を設定する `DisposableEffect` を実装。

13. **WebRTC Stats リアルタイム表示**
    - `WebRtcManager` にて `PeerConnection.getStats()` を 1 秒間隔で定期実行し、`inbound-rtp` レポートから FPS / Bitrate / Loss / frameWidth / frameHeight を取得。
    - `WebRtcStats` データクラスを `StateFlow` で公開し、`DebugPanel` でリアルタイム表示。デバイス画面サイズとストリーム解像度も表示。

14. **接続画面の刷新 (Web クライアント準拠デザイン)**
    - RN版 `ConnectForm.tsx` から Web 版 `index.tsx` に準拠したデザインへ刷新。中央配置の大型アイコン、ステータスインジケーター、「Connect」ボタンを配置。
    - Session ID / Codec 等の接続設定をダイアログに集約し、メイン画面をシンプルに維持。

15. **パフォーマンス最適化**
    - Gallery 画面のサムネイルにダウンサンプリングを適用 (`Coil` の `size()` 指定)。
    - `GalleryViewModel` でリスト構築ロジック (Justified レイアウト計算) を実行し、UI スレッドの負荷を軽減。
    - `SharedTransitionLayout` を Gallery ↔ GalleryDetail 間に限定し、Viewer 画面 (WebRTC 毎フレームレンダリング) を除外してフレームドロップを回避。

16. **映像操作ジェスチャー（ピンチズーム / パン / ダブルタップリセット）**
    - RN版 `VideoPlayer.tsx` の `Gesture.Pinch` + `Gesture.Pan` + `Gesture.Tap(numberOfTaps: 2)` に相当するジェスチャーハンドラを `ViewerScreen.kt` に実装。
    - **ピンチズーム**: `detectTransformGestures` で zoom ファクターを検出し、`graphicsLayer.scaleX/scaleY` に反映。最小値 1.0 を保証し、等倍以下への縮小を防止。
    - **パン**: ズーム中 (`scale > 1`) のみ `graphicsLayer.translationX/translationY` でドラッグ移動。等倍時はパン不可。
    - **ダブルタップリセット**: `detectTapGestures(onDoubleTap)` で `Animatable` を使用し、`scale` と `offset` をスムーズにアニメーション付きで `1f` / `Offset.Zero` にリセット。
    - **シングルタップとの共存**: 既存の `clickable` を `detectTapGestures(onTap, onDoubleTap)` に統合。Compose の `detectTapGestures` はダブルタップ判定を自動で行うため、RN版のような `requireExternalGestureToFail` は不要。

---

## 📊 RN ↔ Kotlin 機能差分マトリクス

以下は `client/apps/mobile` (React Native) と `android` (Kotlin) の機能ごとの実装状況を比較したものです。

| カテゴリ | 機能 | RN (`client/apps/mobile`) | Kotlin (`android`) | 備考 |
|---------|------|:---:|:---:|------|
| **接続** | ConnectScreen (URL入力 + 接続ボタン) | ✅ | ✅ | Card ベースの RN 準拠デザイン。Session ID 入力 + Connect / View Gallery ボタン |
| **接続** | 接続ステータス表示 | ✅ | ✅ | |
| **接続** | codec 選択 | — | ✅ | h264 / vp8 / vp9 / av1 ドロップダウン。RN 版にはない Kotlin 独自機能 |
| **接続** | 接続画面からギャラリーを開く | ✅ | ✅ | View Gallery ボタン配置済み、遷移可能 |
| **映像** | WebRTC 映像ストリーミング表示 | ✅ | ✅ | |
| **映像** | object-fit: contain 表示 | ✅ | ✅ | `wrapContentSize` で AT_MOST 制約を与え contain 挙動を実現 |
| **映像** | ピンチズーム / パン | ✅ | ✅ | `detectTransformGestures` + `graphicsLayer` で実装 |
| **映像** | ダブルタップでズームリセット | ✅ | ✅ | `detectTapGestures(onDoubleTap)` + `Animatable` で実装 |
| **映像** | タップでオーバーレイ表示切替 | ✅ | ✅ | `AnimatedVisibility(fadeIn/fadeOut)` で実装 |
| **映像** | オーバーレイ自動非表示 (4秒) | ✅ | ✅ | `LaunchedEffect` + `delay(4000)` で実装 |
| **オーバーレイ** | ステータスバッジ (接続状態 + loss) | ✅ | ✅ | green/yellow インジケーター + 実測 loss バッジ |
| **オーバーレイ** | 戻るボタン (切断 + ナビゲーション) | ✅ | ✅ | `ArrowBack` アイコン、切断してナビゲーション |
| **オーバーレイ** | デバッグパネル (FPS/Bitrate/Loss/サイズ) | ✅ | ✅ | リアルタイム Stats 値と Device/Stream 画像解像度を表示 |
| **オーバーレイ** | 設定パネル (Audio volume mock 等) | ✅ | ✅ | デザイン実装済み（モック値）。Audio スライダー + Disconnect ボタン |
| **オーバーレイ** | ギャラリーモーダルのオープン | ✅ | ✅ | ボタンからギャラリーへ遷移可能 |
| **オーバーレイ** | スクリーンショットボタン | ✅ | ✅ | ボタンから撮影可能 (Toast 表示) |
| **スクリーンショット** | DataChannel 経由でリクエスト送信 | ✅ | ✅ | `takeScreenshot()` にてリクエスト送信実装 |
| **スクリーンショット** | バイナリチャンク受信・結合・保存 | ✅ | ✅ | `ScreenshotProcessor` にて順序保証と結合を実装 |
| **スクリーンショット** | MediaStore / ギャラリーへの保存 | ✅ | ✅ | `ScreenshotRepository` 経由で MediaStore へ保存 |
| **スクリーンショット** | フラッシュ + サムネイル縮小アニメーション | ✅ | ✅ | `ScreenshotFlash.kt` にて実装済み。白フラッシュ + 画像縮小移動 + フェードアウト |
| **スクリーンショット** | スクリーンショット ID ↔ ローカル ID マッピング (DB) | ✅ | ✅ | Room Entity / DAO および Repository 連携実装 |
| **分析** | AnalyzeRequest 送信 (DataChannel) | ✅ | ✅ | `requestAnalyze()` にて送信 |
| **分析** | AnalyzeResponse ストリーミング受信 (chunk → done) | ✅ | ✅ | `DataChannel` より受信・結合 |
| **分析** | 分析結果の DB 保存 | ✅ | ✅ | `AnalysisDao` を通じて保存・自動ロード |
| **ギャラリー** | ギャラリー画面 (全画面 / モーダル) | ✅ | ✅ | `GalleryScreen.kt` 実装 (LazyColumn + JustifiedRow) |
| **ギャラリー** | Justified レイアウト・日付セクション | ✅ | ✅ | アスペクト比に基づく行分割と日付ヘッダーを実装 |
| **ギャラリー** | ゲームタイトルフィルター | ✅ | ✅ | 横スクロールの Title Cards およびフィルタ表示 |
| **ギャラリー** | 検索パネル (テキスト / 日付 / お気に入りフィルタ) | ✅ | ✅ | `SearchPanel.kt` にて各種フィルタとチップ UI を実装 |
| **ギャラリー** | スクリーンショット詳細 (カルーセル) | ✅ | ✅ | `GalleryDetailScreen.kt` (RN準拠デザイン: Header/Actions/右スライドInfoPanel) |
| **ギャラリー** | 詳細: InfoPanel 連動の画像縮小アニメーション | ✅ | ✅ | `Animatable` で画像エリア幅を InfoPanel 開閉に連動してアニメーション |
| **ギャラリー** | 詳細: 上下スワイプで一覧に戻る | ✅ | ✅ | `detectVerticalDragGestures` + スケール縮小 + 背景透過 + 閾値判定 |
| **ギャラリー** | お気に入り切替 | ✅ | ✅ | 一覧および詳細画面で切替可能 |
| **ギャラリー** | 共有機能 (Twitter / 汎用) | ✅ | ✅ | Intent.ACTION_SEND を使用して実装 |
| **ギャラリー** | 削除機能 | ✅ | ✅ | 詳細画面から削除して一覧へ戻る |
| **ギャラリー** | 分析ビューアー (VN シーン解析表示) | ✅ | ✅ | 詳細画面内に `AnalysisViewer` を実装し、Scene/Dialogue/Characters を表示 |
| **PiP** | Picture-in-Picture モード | ✅ | ✅ | RN版は Expo Native Module として実装 (`modules/pip/`) |
| **PiP** | 自動 PiP (アプリ離脱時) | ✅ | ✅ | `setAutoEnterEnabled` を使用して実装済み |
| **PiP** | PiP ソース矩形計算 | ✅ | ✅ | 映像のアスペクト比 (Rational) を動的に計算して設定 |
| **PiP** | PiP 中の UI 制御 (オーバーレイ非表示等) | ✅ | ✅ | isInPipMode を監視して UI を切り替え |
| **統計** | WebRTC Stats (FPS/Bitrate/Loss) 定期取得 | ✅ | ✅ | `PeerConnection.getStats()` を定期実行して表示 |
| **入力** | タッチ入力の DataChannel 送信 | ❌ | ❌ | 両方未実装 |
| **UI** | 画面回転アンロック | ✅ | — | RN版は Expo のデフォルト portrait ロック解除用。Kotlin版は `screenOrientation` 未指定のため既にOS設定に従う |
| **DB** | Room Entity 定義 (3テーブル) | ✅ | ✅ | |
| **DB** | DAO / クエリ定義 | ✅ | ✅ | |
| **DB** | DB サービス層 (screenshot-service 等) | ✅ | ✅ | `ScreenshotRepository` として実装し Hilt で注入 |
| **DI** | Hilt モジュール構成 | — | ✅ | `DatabaseModule`, `WebRtcModule`, `RepositoryModule` |
| **テーマ** | Material3 テーマ (Color/Type/Theme) | — | ✅ | 常時ダークテーマ (zinc ベース) に統一 |

**凡例**: ✅ = 実装済み、🔶 = 部分的に実装（基盤のみ）、❌ = 未実装

### 概算まとめ

| 指標 | RN | Kotlin |
|------|:---:|:---:|
| **画面数** | 3 (Connect / Viewer / Gallery) | 3 (Connect / Viewer / Gallery) |
| **機能的な実装率 (Kotlin/RN)** | — | **約 100%** |
| **残る未実装機能** | — | なし（タッチ入力送信は両プラットフォーム共に未実装） |

---

## ⚠️ 次のステップ・残課題 (Next Steps)

### その他
1.  **`client/apps/mobile` の削除**: Kotlin 側で RN と同等以上の機能が揃ったため、React Native フォルダを完全に削除する。
2.  **タッチ入力の送信**: DataChannel 経由でタッチ/クリック入力を hostd に送信する機能の実装（両プラットフォーム共に未実装）。

> [!NOTE]
> `android` フォルダ内で `Android Studio` を起動し、実機やエミュレータに対して `Run` を実行することで、Kotlin ネイティブアプリが起動します。ConnectScreen で Session ID を入力し（デフォルト: `fixed`）、codec を選択して Connect を押すと、`ws://10.0.2.2:8787/api/signal?session_id={id}&role=viewer` に接続し、hostd からの映像がストリーミング表示されます。
