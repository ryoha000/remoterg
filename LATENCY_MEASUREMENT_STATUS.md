# E2E レイテンシ計測 — 現状と試行記録

## 元々の問題

`LATENCY_MEASUREMENT.md` に基づき、DataChannel 上の NTP 風時刻同期で hostd と Android の monotonic clock オフセットを推定し、`t_cap`（hostd 側キャプチャ時刻）と `t_render`（Android 側描画時刻）から E2E レイテンシを算出していた。

しかし、**DataChannel の `frame_sample` メッセージと VideoFrame の 1:1 対応が不安定**という根本的な信頼性の問題があった。

- DataChannel メッセージと RTP ビデオフレームは異なる経路で伝送される
- フレームドロップ・バッファリング・再送などで順序・数がずれる
- `VideoFrame.timestampNs`（受信側 monotonic）と `t_cap`（送信側 monotonic）の近接マッチング（15ms 閾値）が不安定

## 試行: abs-capture-time RTP 拡張ヘッダーの活用

### 方針

RTP パケットに直接埋め込まれた abs-capture-time ヘッダーを利用すれば、DataChannel メッセージとのマッチングが不要になる。

- **Rust 側**: `write_sample_with_extensions` で abs-capture-time 拡張ヘッダー（NTP UQ32.32）を RTP パケットに付与（既存実装済み）
- **Kotlin 側**: `VideoDecoderFactory` / `VideoDecoder` をラップし、デコード前の `EncodedImage.captureTimeNs` を傍受して壁時計ベースの E2E を算出

### 実装した内容

1. **`LatencyDecoderFactory`** — `DefaultVideoDecoderFactory` をラップし、各デコーダを `LatencyVideoDecoder` でラップ
2. **`LatencyVideoDecoder`** — `VideoDecoder` の委譲ラッパー。`decode()` 時に `EncodedImage.captureTimeNs` を `CaptureTimeStore`（FIFO キュー）に push
3. **`CaptureTimeStore`** — デコード順で captureTimeNs を保持し、`onFrameRendered()` で poll して対応づける
4. **`NtpUtils`** — NTP UQ32.32 → Unix ms 変換、captureTimeNs → Unix ms 変換
5. **2段フォールバック**:
   - 方法A: `CaptureTimeStore` から `captureTimeNs` を取得 → 壁時計に変換 → `System.currentTimeMillis() - captureUnixMs` で E2E 算出
   - 方法B: DataChannel `frame_sample` の `capture_unix_ms` を直接使用

### 発生した問題

#### 問題1: WrappedNativeVideoDecoder のクラッシュ

`DefaultVideoDecoderFactory.createDecoder()` が返すデコーダは `WrappedNativeVideoDecoder` であり、**すべての Java メソッド** (`initDecode`, `decode`, `release`, `getImplementationName`) が `UnsupportedOperationException` をスローする。

WebRTC のネイティブ (C++) レイヤーは `WrappedNativeVideoDecoder` を検出すると Java インタフェースを呼ばず C++ 側を直接呼ぶ設計だが、`LatencyVideoDecoder` でラップするとその検出が効かなくなり、Java 経由で呼び出されてクラッシュ（JNI `ExceptionCheck` → `SIGABRT`）する。

**対処**: `createDecoder()` で `WrappedNativeVideoDecoder` の場合はラップをスキップし、そのまま返すようにした。

**結果**: 方法A は機能しない。ネイティブデコーダが使われる限り `CaptureTimeStore` にデータが入らない。

#### 問題2: NTP タイムスタンプの JSON オーバーフロー

DataChannel の `frame_sample` で送信していた `ntp_timestamp: u64`（NTP UQ32.32 形式）が、2026年現在では `~1.7 × 10^19` となり Java の `Long.MAX_VALUE` (`~9.2 × 10^18`) を超過。JSON 数値として `getLong()` でパースするとオーバーフローし、レイテンシが 9999ms 固定になった。

**対処**: `ntp_timestamp: u64` を `capture_unix_ms: i64`（Unix ミリ秒）に変更し、JSON で安全に送受信できるようにした。

## 現在の状態

| 方法 | 状態 | 備考 |
|------|------|------|
| 方法A (captureTimeNs) | 不動作 | WrappedNativeVideoDecoder はラップ不可。ネイティブデコーダでは captureTimeNs を傍受できない |
| 方法B (capture_unix_ms via DataChannel) | **無効化（2026-03-05）** | フレーム-メッセージの 1:1 対応が不安定なため、表示値更新には使用しない方針へ変更 |
| 方法C (ntp_time_ms via LatencyNativeSink) | **不成立** | SDP交渉と送信側 ext 付与は成功するが、Android 受信側で `VideoFrame.ntp_time_ms` が常に未設定（`-1`）で目的未達 |
| 方法D (`packet_infos.absolute_capture_time` via NativeSink) | **実装・調整中（2026-03-05）** | C++ `VideoFrame.packet_infos()` 直接読取は動作確認済み。`local_capture_clock_offset` 欠落時の補正・C経路のみの更新方針へ調整継続中 |

## 調査: RTP 拡張ヘッダーを Java から直接読めないか

libwebrtc の C++ ソースコードを追跡し、デコード済みフレームが Java に渡るまでのパイプラインを調査した。

### C++ 側のデータフロー

```
RTP パケット (abs-capture-time 拡張)
  → EncodedImage.ntp_time_ms_ にセット
  → generic_decoder.cc: decodedImage.set_ntp_time_ms(frameInfo->ntp_time_ms)  ← NTP キャプチャ時刻
  → video_stream_decoder_impl.cc: decoded_image.set_timestamp_us(frame_info->render_time_us)  ← レンダリング予定時刻
  → NativeToJavaVideoFrame(): frame.timestamp_us() * kNumNanosecsPerMicrosec → Java timestampNs
```

### C++ VideoFrame のフィールドと Java への伝達

| C++ フィールド | 内容 | Java に渡るか |
|---|---|---|
| `timestamp_us` | ジッタバッファが算出した**レンダリング予定時刻**（受信側ローカル時刻基準） | `VideoFrame.timestampNs` として渡る |
| `ntp_time_ms` | 送信側の **NTP キャプチャ時刻**（求めていた値そのもの） | **渡らない** |

### 根拠コード

`sdk/android/src/jni/video_frame.cc` の `NativeToJavaVideoFrame`:

```cpp
return Java_VideoFrame_Constructor(
    jni, j_video_frame_buffer, static_cast<jint>(frame.rotation()),
    static_cast<jlong>(frame.timestamp_us() *    // ← timestamp_us のみ
                       rtc::kNumNanosecsPerMicrosec));
```

`ntp_time_ms()` は一切参照されずに捨てられている。

### 結論

標準の Java API だけでは `ntp_time_ms` や `packet_infos` にアクセスできない。ただし **`ntp_time_ms` を使わなくても、デコード後の C++ `VideoFrame` には `packet_infos` が伝播しており、そこから `absolute_capture_time` を直接読める**。

## 追加調査: packet_infos 経由の absolute_capture_time

### データの伝播パス

```
RTP パケット受信 (rtp_video_stream_receiver2.cc)
  → RtpPacketInfo に absolute_capture_time + local_capture_clock_offset をセット
  → EncodedFrame.PacketInfos() に格納
  → generic_decoder.cc: decodedImage.set_packet_infos(frame_info->packet_infos)
  → C++ VideoFrame.packet_infos() で利用可能  ← ★ ここでアクセス可能
  → NativeToJavaVideoFrame() で Java に変換される際に packet_infos は落とされる
```

### C++ 側で利用可能なフィールド

`RtpPacketInfo`（`api/rtp_packet_info.h`）:

| フィールド | 型 | 内容 |
|---|---|---|
| `absolute_capture_time()` | `optional<AbsoluteCaptureTime>` | NTP UQ32.32 のキャプチャ時刻 + 推定クロックオフセット |
| `local_capture_clock_offset()` | `optional<TimeDelta>` | ローカル時計とキャプチャ側時計のオフセット |
| `receive_time()` | `Timestamp` | パケット受信時刻（ローカル Clock ベース） |

`AbsoluteCaptureTime`（`api/rtp_headers.h`）:

| フィールド | 型 | 内容 |
|---|---|---|
| `absolute_capture_timestamp` | `uint64_t` | NTP UQ32.32 キャプチャ時刻 |
| `estimated_capture_clock_offset` | `optional<int64_t>` | 送信者とキャプチャ側の時計オフセット |

### ローカル時刻基準のキャプチャ時刻算出式

libwebrtc 内部（`rtp_video_stream_receiver2.cc`）で使われている計算式:

```cpp
capture_time_local_ms = NtpTime(act.absolute_capture_timestamp).ToMs()
                      + local_capture_clock_offset.ms();
```

`local_capture_clock_offset` は libwebrtc の `CaptureClockOffsetUpdater` が abs-capture-time の `estimated_capture_clock_offset` をもとにローカル NTP 時計との差を算出したもの。これにより **ローカルの壁時計基準でのキャプチャ時刻** が得られる。

## 方針: カスタム JNI VideoSink によるフレーム単位 abs-capture-time 取得

### アーキテクチャ

```
VideoTrack (C++ native)
  ├─ 標準 VideoSinkWrapper → Java VideoSink → 描画パイプライン（既存）
  └─ LatencyVideoSink (カスタム C++) → JNI callback → Kotlin（キャプチャ時刻のみ）
```

`VideoTrack` は複数の `VideoSink` を持てる。標準の描画用 Sink はそのまま残し、キャプチャ時刻取得専用の C++ Sink を追加する。

### なぜ libwebrtc をリビルドせずに動くか

- `VideoSinkInterface<VideoFrame>` は**ヘッダのみの純粋仮想テンプレート**
- `VideoTrackInterface::AddOrUpdateSink()` は**仮想関数**。`getNativeVideoTrack()` で取得したポインタから vtable 経由で呼べる。WebRTC .so とのリンクは不要
- `VideoFrame::packet_infos()` は**インライン関数**（ヘッダで定義）
- `RtpPacketInfo::absolute_capture_time()` / `local_capture_clock_offset()` も**インライン**

### 主要コンポーネント

**1. C++ LatencyVideoSink (`latency_sink.cc`)**

```cpp
class LatencyVideoSink : public webrtc::VideoSinkInterface<webrtc::VideoFrame> {
  void OnFrame(const webrtc::VideoFrame& frame) override {
    const auto& infos = frame.packet_infos();
    if (infos.empty()) return;
    const auto& act = infos.front().absolute_capture_time();
    if (!act.has_value()) return;

    int64_t capture_ntp_ms = webrtc::NtpTime(act->absolute_capture_timestamp).ToMs();

    const auto& offset = infos.front().local_capture_clock_offset();
    if (offset.has_value()) {
      capture_ntp_ms += offset->ms();  // ローカル NTP 時計基準に変換
    }

    // JNI で Kotlin へコールバック
    CallJava(capture_ntp_ms, frame.timestamp_us());
  }
};
```

**2. JNI ブリッジ関数**

| 関数 | 役割 |
|---|---|
| `nativeCreateLatencySink(callback)` → `jlong` | LatencyVideoSink を生成、Java callback を保持 |
| `nativeAttachToTrack(nativeTrack, nativeSink)` | `AddOrUpdateSink` で VideoTrack に登録 |
| `nativeDetachAndDestroy(nativeTrack, nativeSink)` | `RemoveSink` + delete |

**3. Kotlin 側**

```kotlin
val nativeTrackPtr = videoTrack.getNativeVideoTrack()  // public API で取得可能
val sinkPtr = nativeCreateLatencySink { captureNtpMs, timestampUs ->
    val e2eMs = System.currentTimeMillis() - captureNtpMs
    lastLatencyMs.set(e2eMs.toInt().coerceIn(0, 9999))
}
nativeAttachToTrack(nativeTrackPtr, sinkPtr)
```

### ビルド要件

- **NDK**: JNI コンパイル用
- **WebRTC ヘッダ**: `api/video/video_frame.h`, `api/video/video_sink_interface.h`, `api/rtp_packet_infos.h`, `api/rtp_packet_info.h`, `api/rtp_headers.h` とその依存ヘッダ
- **WebRTC .so とのリンク**: 原則不要（仮想関数とインライン関数のみ使用）。ただし `RtpPacketInfo` のコンストラクタや `NtpTime::ToMs()` がヘッダ定義でない場合、該当 `.cc` ファイルを自前のビルドに含める必要がある

### メリット

- フレームごとに確実に abs-capture-time を取得できる（FIFO マッチング不要）
- DataChannel との同期問題が完全に解消される
- libwebrtc の AAR をリビルドする必要がない
- `local_capture_clock_offset` を使えばクロック同期の問題も WebRTC 内部で解決済み

## 旧実装完了: JNI LatencyVideoSink（2025-03）

> 注: この節は `ntp_time_ms` / メモリオフセット読取ベースの旧実装記録。2026-03-05 に `packet_infos.absolute_capture_time` 直接読取（方法D）へ更新済み。

### 作成ファイル

- `android/app/src/main/cpp/webrtc_stubs.h` — WebRTC M114 ABI 互換の型スタブ
- `android/app/src/main/cpp/latency_sink.cpp` — LatencyVideoSink 実装と JNI ブリッジ
- `android/app/src/main/cpp/CMakeLists.txt` — NDK ビルド設定
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt` — Kotlin ブリッジ

### 変更ファイル

- `android/app/build.gradle.kts` — NDK / CMake 設定追加
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/WebRtcManager.kt` — LatencyNativeSink 統合

### 動作

1. `onTrack` で VideoTrack 取得時に `LatencyNativeSink.attachToTrack()` を呼び出し
2. C++ `LatencyVideoSink` が `OnFrame` で VideoFrame の `ntp_time_ms` を**オフセットベース**で読み取り（ABI 互換問題を回避）
3. JNI コールバックで `captureUnixMs` を Kotlin に渡し、`E2E = System.currentTimeMillis() - captureUnixMs` で算出
4. 方法B（DataChannel `frame_sample`）は従来どおりフォールバックとして維持

### SIGSEGV 修正（2025-03）

`packet_infos` やメンバ経由の `ntp_time_ms` アクセスで ABI 不一致によるクラッシュが発生したため、オフセットベースの直接読み取りに変更。M114 VideoFrame の ntp_time_ms_ は offset 24、timestamp_us_ は offset 32（64-bit）。`packet_infos` の abs-capture-time は構造が複雑なため未使用。

### SIGSEGV vtable 修正（2026-03）

**原因**: M114 の `rtc::VideoSinkInterface` には `OnConstraintsChanged` 仮想関数があるが、スタブには存在せず vtable エントリ数が不一致。WebRTC が `OnConstraintsChanged` を呼ぶと vtable 境界を超えたアドレスにジャンプして SEGV_ACCERR が発生。

**修正**:
1. `webrtc_stubs.h`: `rtc` 名前空間に `VideoSinkInterface` を定義し、`OnConstraintsChanged` を追加して vtable を M114 に合わせた
2. `latency_sink.cpp`: 基底クラスを `rtc::VideoSinkInterface` に変更し、`OnConstraintsChanged` を空実装でオーバーライド
3. JNI: `CallJava` 内の `DetachCurrentThread()` を削除。IncomingVideoSt スレッドを Detach すると後続の WebRTC VideoSinkWrapper 呼び出しでクラッシュするため
4. `thread_local JNIEnv*` キャッシュで毎フレームの `GetEnv` 呼び出しを効率化

## 追加観測: `Latency[B]` 負値（2026-03-05）

### 観測ログ

```
Latency[B]: e2e=-72ms captureUnixMs=1772708904306 tRender=1772708904234
```

### 現時点の解釈

- `E2E = tRender - captureUnixMs` が負値になるのは物理的に不自然で、**方法B（DataChannel フォールバック）の相関ずれ or 時計基準ずれ**が残っている可能性が高い
- したがって、方法Bの値はデバッグ用の参考値として扱い、主計測値には採用しない

### 方針（今回）

- **今は修正しない**
- （当時）主計測は方法Cを継続していたが、2026-03-05 の追加調査で方法C不成立が確定し、現行方針は方法Dへ更新

## 追加観測: `ntp_time_ms` 経路は不成立（2026-03-05）

### 事実

1. SDP 交渉は成立
   - Android: `Latency[ACT-SDP:local-offer]` / `Latency[ACT-SDP:remote-answer]` で abs-capture-time extmap を確認
   - hostd も同様に extmap を確認
2. 送信側 RTP 拡張付与は成立
   - hostd: `ACT[send] ... ext_count=1` を継続確認
3. 受信側 `ntp_time_ms` 読み取りのみ失敗
   - Android JNI: `ACT skip ... status=2 success=0`
   - 詳細: `c0_raw=-1` 固定、`c1_raw` は時間進行に応じて増加（`timestamp_us` 相当）
   - `ACT scan ... no plausible ntp-like value in first 192 bytes`

### 結論

- **`VideoFrame.ntp_time_ms` を読むアプローチ（方法C）では目的を達成できない**
- 現在の経路では、ACT がネゴシエート・送信されていても `ntp_time_ms` へ反映されないため、E2E の主計測値として利用不能

### 実装結果（2026-03-05）

- `ntp_time_ms` 依存を廃止し、**`VideoFrame.packet_infos().absolute_capture_time` を直接読む**方式へ切替済み
- C++ 実装は `latency_sink.cpp` の `ExtractCaptureTimeFromFrame` を全面差し替え:
  - `frame.packet_infos()` を走査し、`absolute_capture_time` を持つエントリを探索
  - `absolute_capture_timestamp(UQ32.32)` を NTP ms に変換
  - `local_capture_clock_offset` がある場合のみ `+ offset.ms()` を適用
  - Unix ms 妥当性チェック（2020-01〜2034-12）を満たす値のみ採用
- `webrtc_stubs.h` は廃止し、`android/app/src/main/cpp/third_party/webrtc_m124/` に M124 ヘッダ群（34ファイル）+ Abseil thin shim を追加
- `CMakeLists.txt` は上記 include ルートを追加し、`latency_sink` のビルドを継続
- ビルド確認:
  - 実行コマンド: `cd android && .\\gradlew.bat :app:externalNativeBuildDebug :app:assembleDebug`
  - 結果: **BUILD SUCCESSFUL**（2026-03-05）
- 未確認事項:
  - 実機 `logcat` で `ACT frame=...` / `Latency[C-native]` の継続更新確認
  - `Latency[C-missing]` が初期以外で増え続けないことの確認

## 追加更新（2026-03-05 夜）

### 依存ライブラリ切替

- Android WebRTC 依存を `io.getstream:stream-webrtc-android` から `io.github.webrtc-sdk:android:125.6422.07` へ切替
- 選定経緯は `WEBRTC_LIBRARY_SELECTION.md` に記録

### Android 側（受信・表示）の変更

- `WebRtcManager` で `timestampUs` を使って native callback と render frame を突合し、**描画時刻基準**で `Latency[C]` を算出
- `Latency[B]`（DataChannel `frame_sample`）での表示値更新を停止
  - 方針: 表示値は `C`（必要時のみ `A`）のみで更新
- `C-native-future` 対策として、`captureUnixMs` の時計先行分を推定して補正
  - `rawMsgLagMs` から `aheadEstimateMs` を推定し、`correctedCaptureUnixMs` を利用
  - 補正は上方向に速く追従、下方向に緩やか減衰
- `C-miss` 時は短時間のみ native callback 側の直近補正値を暫定採用
  - `B` 経路へのフォールバックは行わない

### C++ JNI 側（`latency_sink.cpp`）の変更

- `local_capture_clock_offset` 欠落時の扱いを見直し
  - 欠落フレームを全面スキップする実装は廃止
  - `absolute_capture_timestamp` 単体でも計測継続
- 取得できた `local_capture_clock_offset` はキャッシュし、後続フレームで再利用可能にした（欠落時の継続性向上）
- デバッグログを拡張
  - `offset_applied`, `offset_cached`, `reuse_count`, `no_local_offset` を可視化

### hostd 側（送信）変更

- `video-stream` の abs-capture-time 拡張を 8byte → 16byte 対応
  - `absolute_capture_timestamp` に加え `estimated_capture_clock_offset` を付与
  - 現実装では sender/capturer 同一前提で `estimated_capture_clock_offset = 0` を設定
- ログを拡張
  - `ACT[send] ... est_offset_q32x32=... ext_bytes=16`

### 現時点の観測まとめ

- `ACT frame success` は継続して増加し、`packet_infos.absolute_capture_time` 経路は稼働
- 一部環境で `offset_applied=0` が継続（`local_capture_clock_offset` 未設定）
- その結果、`C-native-future` が散発しうるため、Android 側で時計先行補正を実施中
- 0ms 固定化は `C` マッチ失敗時の更新停止が主因で、突合条件と更新ロジックを調整中
