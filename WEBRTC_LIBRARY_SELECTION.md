# WebRTC ライブラリ選定メモ（2026-03-05）

## 結論

Android クライアントの WebRTC 依存を `io.getstream:stream-webrtc-android` から `io.github.webrtc-sdk:android` へ切り替える。

- 採用バージョン: `io.github.webrtc-sdk:android:125.6422.07`
- 変更日: 2026-03-05
- 主目的: `packet_infos.absolute_capture_time` を JNI で読む実装における ABI 不一致リスク低減と、バージョン追跡性の向上

## 背景

E2E レイテンシ計測の主経路を以下へ移行した。

- C++ `VideoFrame::packet_infos()` から `absolute_capture_time` を取得
- `local_capture_clock_offset` を適用してローカル時刻基準へ変換
- Kotlin へ JNI callback で渡して `System.currentTimeMillis()` と差分計算

この変更後、実機で `IncomingVideoSt` スレッドにて SIGSEGV が発生した。

ログ例（2026-03-05）:

- `Latency[C-missing]: native packet_infos callback not received yet`
- `Fatal signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), tid: IncomingVideoSt`

`packet_infos` は C++ 側の内部構造と ABI に強く依存するため、AAR バイナリと JNI 側ヘッダの不整合があるとクラッシュしやすい。

## これまでの経緯（時系列）

1. 旧方式（DataChannel `frame_sample`）を使用
2. `ntp_time_ms` 経路を試行したが実受信で未設定（`-1`）が継続
3. `packet_infos.absolute_capture_time` 直接読取へ切替
4. `stream-webrtc-android` 利用下で SIGSEGV を観測
5. 依存ライブラリの再選定を実施
6. `webrtc-sdk` へ移行を決定

## 候補比較（要点）

### io.getstream:stream-webrtc-android

- 利点:
  - 既存プロジェクトで利用実績あり
  - `org.webrtc` API 互換
- 懸念:
  - JNI で `packet_infos` を扱う場合、内部 ABI を厳密に合わせるための追跡が難しい

### io.github.webrtc-sdk:android（採用）

- 利点:
  - WebRTC 系バージョンが明示的で、リビジョン追跡しやすい
  - `org.webrtc` API 互換のためアプリ側 Kotlin 変更が最小
  - Maven / GitHub の更新情報が明確
- 懸念:
  - `absolute_capture_time` が Java API に直接露出されるわけではない
  - JNI 側ヘッダとの整合管理は引き続き必要

### その他候補

- `com.dafruits:webrtc`: メンテ停止明記があり、長期運用の候補から除外
- `ch.threema:webrtc-android`: 独自パッチ適用版のため ABI 差分調査コストが高い
- `im.conversations.webrtc:webrtc-android`: 更新頻度と情報量の点で優先度を下げた

## 技術的な整理

重要点は「どの配布を使うか」よりも以下。

- `AbsoluteCaptureTime` は C++ 側データであり、Java API からは直接取得しにくい
- そのため JNI 実装（`latency_sink.cpp`）で `packet_infos` を読む必要がある
- このとき AAR バイナリと参照ヘッダの ABI 整合が必須

つまり、ライブラリ変更は問題の直接解決ではなく、整合管理をしやすくするための施策である。

## 今回の実施内容

- `android/gradle/libs.versions.toml`
  - `webrtc` を `io.github.webrtc-sdk:android:125.6422.07` に変更
- `android/README.md`
  - WebRTC 依存表記を新ライブラリへ更新

## 残課題

- JNI 側で参照している `android/app/src/main/cpp/third_party/webrtc_m124` は M124 ヘッダであり、現在の採用版（125.6422.07）と完全一致ではない
- クラッシュ再発防止のため、ヘッダを採用 AAR と同一リビジョンに合わせる追作業が必要

## 検証観点（次回）

- 起動後に `Latency[C-native]` が継続して更新されること
- `Latency[C-missing]` が初期数回で収束すること
- `IncomingVideoSt` スレッドで SIGSEGV が再発しないこと
- 長時間視聴時にネイティブクラッシュが発生しないこと
