# WebRTC レイテンシ計測記事 構成案

## 記事の主題
- WebRTC ベースの映像配信アプリで、映像 E2E レイテンシを測るためにどのような計測アーキテクチャを設計したかを書く
- 主題は試行錯誤の記録ではなく、**WebRTC のレイテンシ計測で何が難しく、どんな責務分担が必要か**の説明
- 特定の実装の紹介に閉じず、WebRTC の測定系一般に通じる設計原則を持ち帰れる記事にする

## タイトル案
- WebRTC の E2E レイテンシはどう測るのか: フレーム単位計測アーキテクチャの設計
- DataChannel だけでは測れない: フレーム単位 E2E 計測基盤の作り方
- WebRTC の遅延計測を支える、フレーム同定と時間軸統一の設計

## 想定読者
- WebRTC を使ったリアルタイム配信・遠隔操作アプリを実装している人
- Android とネイティブ層をまたいだ計測基盤を設計したい人
- 測定値の意味と成立条件を厳密に扱いたい人

## この記事で前に出したい価値
- WebRTC で遅延を測る難しさは、計算式そのものではなく**フレーム同定**にある
- DataChannel と RTP が別経路である以上、測定系にもアーキテクチャが要る
- Java API だけでは完結しないため、**どこで `VideoFrame` を取得するか**まで設計しないといけない

## Abstract
WebRTC ベースの映像配信アプリで E2E レイテンシを測ろうとすると、単に送受信の時刻を記録するだけでは足りない。送信側で取得したフレームと、受信側で表示されたフレームを正しく対応づけ、さらに両者を同じ時間軸で比較する必要があるからだ。

しかし WebRTC では、映像本体は RTP、補助的なメタデータは DataChannel と経路が分かれている。加えて Android の Java API だけでは、フレーム同定に必要な情報を十分に取得できない。そこで本稿では、時刻同期、フレーム同定、ネイティブでの `VideoFrame` 取得、最終計算を分離した計測アーキテクチャを扱う。その全体像と、なぜその役割分担が必要になるのかを説明する。

補足:
- 本稿で扱う E2E は、成立条件を満たして対応づけできたフレームに対する厳密な測定値であり、すべてのフレームに対する近似値ではない

## 1. 何を測りたかったのか

### 1.1 測りたいもの
- 測りたいのは、送信側でキャプチャされたフレームが Android クライアントで表示されるまでの時間
- 起点は送信側のキャプチャ時刻
- 終点は受信側の描画時刻
- つまり知りたいのは、あるフレームが「取られてから表示されるまで」の時間

### 1.2 でも難しいのは引き算そのものではない
- やりたいこと自体は「表示された時刻から、取られた時刻を引く」だけに見える
- しかし実際には、送信側のキャプチャ時刻と受信側の描画時刻をそのまま比較できない
- 加えて、送信側で記録した時刻と受信側で取得したフレームを簡単に紐づけることもできない
- 計測を成立させるには、次の 2 条件が必要になる
  - その 2 つの時刻を**同じ時間軸**で比較できること
  - 送信側と受信側で**同じフレーム**を見つけられること

## 2. なぜ素朴には測れないのか

### 2.1 送信側と Android の時計は別
- キャプチャ時刻は送信側の monotonic
- 描画時刻は Android の monotonic
- そのままでは引き算できない
- だから clock offset を別途推定する必要がある

### 2.2 同じフレームを特定することが難しい
- E2E レイテンシは、送信側でそのフレームがいつキャプチャされたかと、受信側でそのフレームがいつ表示されたかの差なので、まず両者が同じフレームを指している必要がある
- 素朴に考えると、Android 側で見えている 1 枚のフレームに対して、「いつのフレームなのか」と「いつ描画されたのか」を結び付けられれば十分に見える
- しかし Android アプリケーションコードから見えているのは、libwebrtc の C++ 側で扱われているフレーム情報のうち Java API へ公開されている部分だけである
- 今回ほしかった送信側 capture 時刻に由来する情報はその公開範囲には含まれていない
- そのため、表示直前のフレームは見えても、それが送信側でいつキャプチャされたフレームなのかは Java API だけでは分からない
- 主計測値を成立させるには、C++ 側で保持されているフレーム情報を取得できる、より下位の層を扱う必要がある

## 3. 採用した計測パイプラインの全体像

### 3.1 計測値が成立するまでの流れ
- この計測では、時計合わせとフレーム合わせが別々に進み、最後に合流する
- DataChannel だけでも RTP だけでも完結しない
- だから、最初に「どの情報がどの経路を流れ、どこで紐づくか」を全体図で示す

### 3.2 図の理解に必要な前提
- 1. `CLOCK_MONOTONIC` と `CLOCK_REALTIME` の違い
  - ここは最重要の前提として先に説明する
  - `CLOCK_REALTIME` は壁時計に近い時刻で、NTP などで補正されうる
  - `CLOCK_MONOTONIC` は単調増加する経過時間用の時刻で、レイテンシ計測に向く
  - 今回の計測では、送信側メタデータにこの 2 系統の情報が混在している
  - そのため client 側では、送信側 `CLOCK_MONOTONIC` を client 側 `CLOCK_MONOTONIC` へ写像する必要がある
  - 後述する `abs-capture-time` には絶対時刻系の値が載る仕様なので、送信フレーム照合では `CLOCK_REALTIME` 系の値を使っている
  - 読者に最初に伝えたいのは、「時刻が 2 種類ある」のではなく、「役割の異なる時間軸が 2 つある」ということ
- 2. `abs-capture-time` が何者か
  - `abs-capture-time` は RTP header extension である
  - 送信側でいつキャプチャされたかに対応する絶対時刻系の情報を RTP の映像パケットに載せて運ぶ
  - ここに入るのは絶対時刻系の値であり、送信フレーム照合では `CLOCK_REALTIME` 系の値として使える
  - 図の理解に必要なのは、「なぜ映像パケットから `CLOCK_REALTIME` 系の値が取れるのか」という点であり、その答えが `abs-capture-time` である
  - あわせて、DataChannel で送るフレームメタデータにも、送信フレーム照合のためにこれと同じ値を入れていることを明示する
- 3. DataChannel を何に使っているのか
  - DataChannel では 2 種類の情報をやり取りする
  - ひとつは時計合わせメッセージの送受信
  - もうひとつは、送信側 `CLOCK_MONOTONIC` のキャプチャ時刻と、送信フレーム照合に使う `abs-capture-time` と同じ `CLOCK_REALTIME` 系の値を含むフレームメタデータの受信
- 4. 「時計合わせ要求 / 応答」で何を推定しているのか
  - sender と client は別マシンなので、`CLOCK_MONOTONIC` の値は直接比較できない
  - そのため DataChannel の往復で、送信側 `CLOCK_MONOTONIC` と client 側 `CLOCK_MONOTONIC` のオフセットを推定する
  - やっていることは NTP と同型の時計合わせであり、往復時間を使ってオフセットを見積もる
  - 必要なら RTT の半分を使う近似で offset を推定する
  - 図の「送信側 `CLOCK_MONOTONIC` のキャプチャ時刻を client 側 `CLOCK_MONOTONIC` に変換」が成り立つ前提がこの時計差である
- 5. フレーム照合が何を意味しているのか
  - 送信フレーム照合では、DataChannel で送ったフレームメタデータと、計測用 `VideoSink` で取得した `VideoFrame` を紐づける
  - 表示フレーム照合では、先ほどの `VideoFrame` と、実際に表示段階へ進んだ render callback を紐づける
  - これにより、「送信時刻」と「表示時刻」を同一フレーム単位で対応づけられる
  - frame ID を単純に直接使うというより、送信フレーム照合では `abs-capture-time` と同じ `CLOCK_REALTIME` 系の値を使い、表示フレーム照合では `VideoFrame.timestampUs` 由来の `CLOCK_MONOTONIC` 系の値を使う
  - 一致しないフレームまで無理に計算しないという方針も、ここで触れておくと分かりやすい
- 6. `VideoTrack` / `VideoSink` / `VideoFrame` / render callback の関係
  - WebRTC 実装に不慣れな読者向けに、ここは先に交通整理する
  - client 側では `VideoTrack` を受け、その先で複数の `VideoSink` にフレームを渡せる
  - 今回は
    - C++ 側で直接 `VideoFrame` を受け取る計測用 `VideoSink` の経路
    - 既存の描画用 `VideoSink` から render callback へ進む経路
    の 2 つを並行に持っている
  - そして最後に、その 2 つの経路を表示フレーム照合で同一フレームへ再び紐づける
- 7. `VideoFrame.timestampUs` が何の時刻なのか
  - 名前だけでは送信時刻に見えやすいので、ここで明示する
  - これは client 側で受信後に扱う `VideoFrame` に載っている時刻であり、送信側キャプチャ時刻そのものではない
  - 本稿では、表示フレーム照合に使う client 側 `CLOCK_MONOTONIC` 系のフレーム時刻として扱う
- 記事中では内部変数名をそのまま見せず、次の呼び方を使う
  - `t_cap`: 送信側 `CLOCK_MONOTONIC` のキャプチャ時刻
  - `offsetMonoMs`: 送信側 `CLOCK_MONOTONIC` と client 側 `CLOCK_MONOTONIC` の時計差推定値
  - `capture_unix_ms`: 送信フレーム照合に使う `abs-capture-time` と同じ `CLOCK_REALTIME` 系の値
  - `timestampUs`: 表示フレーム照合に使う client 側 `CLOCK_MONOTONIC` 系のフレーム時刻
  - `frame_sample`: フレームメタデータ
  - `sync_req` / `sync_res`: 時計合わせメッセージ

### 3.3 mermaid 図
- 図の意図:
  - 時計合わせの系統とフレーム同定の系統が並行して進み、最後に E2E 計算へ合流することを一目で見せる
  - DataChannel 上で流れる「時計合わせメッセージ」と「フレームメタデータ」を分けて描く
  - `sync_req` と `sync_res` の往復がないと時計差推定値が作れないことを図に含める
  - 各処理が送信側と client 側のどちらで実行されるかを分かるようにする
- mermaid 案:

```mermaid
flowchart LR
    subgraph Sender["送信側"]
        direction TB

        subgraph SenderDC["送信側 DataChannel 処理"]
            SDC1[時計合わせ要求を受信]
            SDC2[時計合わせ応答を返す]
            SDC3[フレームメタデータを送信<br/>送信側 CLOCK_MONOTONIC のキャプチャ時刻 /<br/>abs-capture-time と同じ CLOCK_REALTIME 系の値]
        end

        subgraph SenderVideo["送信側 映像処理"]
            SV1[フレームをキャプチャ]
            SV2[RTP 映像<br/>with abs-capture-time]
        end
    end

    subgraph Client["client 側"]
        direction TB

        subgraph ClientDC["client 側 DataChannel 処理"]
            CDC1[時計合わせ要求を送信]
            CDC2[時計合わせ応答を受信]
            CDC3[CLOCK_MONOTONIC の時計差推定値を更新]
            CDC4[フレームメタデータを受信]
            CDC5[送信フレーム照合<br/>フレームメタデータと VideoFrame を紐づける]
        end

        subgraph ClientTrack["client 側 受信トラック"]
            CT1[受信した VideoTrack]
        end

        subgraph ClientVideo["client 側 `VideoFrame` 取得"]
            CV1[計測用 VideoSink で<br/>C++ VideoFrame を取得]
            CV2[abs-capture-time から<br/>CLOCK_REALTIME 系の値を取り出す]
            CV3[`VideoFrame.timestampUs` から<br/>CLOCK_MONOTONIC 系の値を取り出す]
        end

        subgraph ClientRender["client 側 描画処理"]
            CR0[既存の描画用 VideoSink]
            CR1[Render callback]
            CR2[表示フレーム照合<br/>VideoFrame と render callback を紐づける]
        end

        subgraph ClientCalc["client 側 最終計算"]
            CC1[送信側 CLOCK_MONOTONIC のキャプチャ時刻を<br/>client 側 CLOCK_MONOTONIC に変換]
            CC2[E2E レイテンシ]
        end
    end

    SV1 --> SDC3
    SV1 --> SV2
    CDC1 --> SDC1
    SDC1 --> SDC2
    SDC2 --> CDC2
    CDC2 --> CDC3
    SDC3 --> CDC4
    CDC4 --> CDC5
    SV2 --> CT1
    CT1 --> CV1
    CT1 --> CR0
    CV1 --> CV2
    CV1 --> CV3
    CV2 --> CDC5
    CR0 --> CR1
    CDC5 --> CR2
    CV3 --> CR2
    CR1 --> CR2
    CDC3 --> CC1
    CR2 --> CC1
    CC1 --> CC2
```

この図は、
- DataChannel で時計差を推定する経路
- RTP / `VideoTrack` 経由でフレームを受け取る経路
- 描画直前の callback で表示時刻を取得する経路
の 3 本を最後に合流させて、送信側キャプチャ時刻を client 側時刻へ変換し、E2E レイテンシを求める流れを表している。

読むときは、まず時計合わせ（`CDC1`〜`CDC3`）、次にフレーム受信と照合（`SV1`〜`CDC5`, `CV1`〜`CV3`）、最後に描画との照合と最終計算（`CR1`〜`CC2`）の順に追うと分かりやすい。

## 4. Java API の外で `VideoFrame` を取得する

### 4.1 Java API には必要な取得点がない
- 第3章で必要だと整理したのは
  - 送信フレーム照合キー
  - 表示フレーム照合キー
  を同時に取れる取得点だった
- しかし Android の Java API には、その 2 つを同時に取れる地点がない
- render callback 側では表示フレーム照合キーは見えるが、送信フレーム照合キーに使う `packet_infos.absolute_capture_time` は見えない
- そこで C++ 側 `OnFrame` から `captureUnixMs` と `timestampUs` を取り出し、Kotlin へ返す構成にした
- そのため、第4章では「どうやって `VideoTrack` 直下の native sink からそれを取るようにしたか」を扱う
- 参照実装:
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/WebRtcManager.kt`
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt`
  - `android/app/src/main/cpp/latency_sink.cpp`

### 4.2 `VideoTrack` に native sink を追加する
- その取得点を作るために、既存の描画経路とは別に計測専用 sink を `VideoTrack` へ追加した
- 今回は Kotlin 側で `track.nativeVideoTrack` を取り出し、JNI で `VideoTrack.nativeAddSink` / `nativeRemoveSink` を呼ぶ構成にした
- C++ 側では `rtc::VideoSinkInterface<webrtc::VideoFrame>` を実装し、`OnFrame` で `VideoFrame` を直接受ける
- 記事では、「公開 API の拡張」ではなく「既存トラックに native sink を横からぶら下げる」という見せ方が分かりやすい
- 強調したいポイント:
  - 描画用経路を壊さずに、計測経路だけを横に増やしている
  - attach / detach / release を Kotlin 側で管理し、失敗時は機能を無効化して本体を巻き込まない
- 参照実装:
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt`
  - `android/app/src/main/cpp/latency_sink.cpp`

### 4.3 配布済みの WebRTC にどう乗るか
- ここでいう `org.webrtc` は、アプリが依存している Android 向け WebRTC ライブラリ
- `AAR` は Android ライブラリの配布形式で、この中に Java / Kotlin から使う API と、内部で動くネイティブライブラリが含まれている
- `JNI` は、その Java / Kotlin 側と C++ 側をつなぐための仕組み
- 今回やりたかったのは、WebRTC を丸ごと自前ビルドして改造することではなく、配布済みの `org.webrtc` をそのまま使いながら、計測用の C++ コードだけを横に追加することだった
- 具体的には
  - Kotlin 側で `VideoTrack` から `nativeVideoTrack` を取り出す
  - `LatencyNativeSink.kt` から JNI を呼び、自前の `latency_sink` を生成する
  - その sink を `VideoTrack.nativeAddSink` で既存トラックへ追加する
  - C++ 側 `OnFrame` で `VideoFrame::packet_infos()` を読み、`captureUnixMs` と `timestampUs` を Kotlin へ返す
  という流れになる
- こう書くと単に sink を足しただけに見えるが、実際には「配布済みライブラリの native 側に、こちらの native コードを正しく接続する」話になっている
- だから難しさの本体は JNI の文法そのものより、既存の WebRTC 実装へ安全に乗ることにあった
- 参照実装:
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt`
  - `android/app/src/main/cpp/latency_sink.cpp`
  - `WEBRTC_LIBRARY_SELECTION.md`

### 4.4 だから ABI とビルド条件まで管理する必要があった
- 上の構成を取る以上、自前の native 側は `org.webrtc` が内部で使っているネイティブライブラリと ABI 前提を揃えて動かす必要がある
- 今回は CMake で WebRTC header を直接参照しつつ、独立した `latency_sink` shared library をビルドしている
- ARM 実機では C++ ABI の整合も必要で、`-fexperimental-relative-c++-abi-vtables` を付けて DSO 境界をまたぐ仮想関数呼び出しを合わせている
- さらに、使う WebRTC 配布の選定自体が ABI リスクの管理になる
  - `org.webrtc` API 互換であること
  - どのリビジョンのバイナリか追跡しやすいこと
  - JNI 側で参照するヘッダと整合を取りやすいこと
- `build.gradle.kts` 側でも ABI 対象と feature flag を持ち、壊れたときに native sink を無効化できるようにしている
- なおスレッド境界や寿命管理は当然必要だが、本文では FFI 実装の補足として短めに流し、主論点はネイティブライブラリ / ABI 整合に置く
- 参照実装:
  - `android/app/src/main/cpp/CMakeLists.txt`
  - `android/app/build.gradle.kts`
  - `WEBRTC_LIBRARY_SELECTION.md`

## 5. まとめ

### 5.1 記事の結論候補
- WebRTC の E2E レイテンシ計測で最初に解くべき問題は、計算式ではなくフレーム同定である
- DataChannel と RTP が別経路である以上、測定系にも役割分担が必要になる
- その制約を分解すると
  - `CLOCK_MONOTONIC` の時刻同期は DataChannel
  - フレーム同定は RTP `abs-capture-time` と同じ `CLOCK_REALTIME` 系の値
  - `VideoFrame` の取得は JNI `VideoSink`
  - 最終計算は `CLOCK_MONOTONIC`
  という構成が自然に導かれる
- 必要なら結語の中で短く、成立条件を満たしたフレームに対して厳密な値を作る構成であることだけ補足する

### 5.2 記事の価値
- 「ある実装ではこうした」だけでなく
- 「WebRTC のレイテンシ計測では、なぜその責務分担が必要になるのか」を説明できる
- 独立した大章というより、短い結語として 2〜4 段落で締める想定

## 6. 記事内で入れたい図
- 図1: 採用アーキテクチャ全体図
- 図1 は mermaid で先に出してもよい
- 時計合わせの系統とフレーム同定の系統が並行し、最後に合流する形を明示する
- 図2: 2 段突合のシーケンス図
- 図3: 時間軸の整理
  - 送信側 `CLOCK_MONOTONIC`
  - client 側 `CLOCK_MONOTONIC`
  - `abs-capture-time` と同じ `CLOCK_REALTIME` 系の値
- 図4: Java API と C++ `VideoFrame` の見える情報の差
- 図5: JNI sink を入れたときの `VideoFrame` 取得位置と責務範囲

## 7. 章ごとに差し込むコード参照候補
- `desktop/services/core/src/lib.rs`
  - `DataChannelMessage::SyncReq`
  - `DataChannelMessage::SyncRes`
  - `DataChannelMessage::FrameSample`
- `desktop/services/webrtc/src/connection.rs`
  - header extension の登録
  - `sync_res` 応答
- `desktop/services/video-stream/src/track_writer.rs`
  - `write_sample_with_extensions`
  - `AbsCaptureTimeExtension`
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyMonotonicMath.kt`
  - offset 推定式
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/WebRtcManager.kt`
  - `handleSyncRes`
  - `handleFrameSample`
  - `deliverMatchedFrameNative`
  - `onFrameRendered`
  - `onTrack` での native sink attach
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/FrameNativeMatchStore.kt`
  - `captureUnixMs` での送信フレーム照合
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt`
  - `track.nativeVideoTrack`
  - sink attach / detach / disable
- `android/app/src/main/cpp/latency_sink.cpp`
  - `ExtractCaptureTimeFromFrame`
  - `LatencyVideoSink::OnFrame`
  - JNI callback
  - thread attach / callback lifetime
- `android/app/src/main/cpp/CMakeLists.txt`
  - ARM ABI 対応
  - isolated JNI shim のビルド設定

## 8. 仕上げ時のチェックリスト
- [ ] 冒頭で「何がそんなに難しいのか」が人間語で伝わる
- [ ] 素朴な方法がどこで壊れるかを序盤で示している
- [ ] 用語の交通整理が早い段階で済んでいる
- [ ] 計測値が成立するまでのパイプラインを先に示してから、実装コラムとして native `VideoSink` の話に入っている
- [ ] 全体図を早めに出している
- [ ] 章立てが `問題設定 -> 素朴には測れない理由 -> 全体パイプラインと図 -> native VideoSink 実装コラム -> まとめ` になっている
- [ ] 送信側 `CLOCK_MONOTONIC`、`abs-capture-time` と同じ `CLOCK_REALTIME` 系の値、ネイティブでの `VideoFrame` 取得、2 段の照合、`CLOCK_MONOTONIC` ベースの最終計算が、第3章の全体図と第4章の実装コラムで役割分担として説明されている
- [ ] 失敗談が日記ではなく設計制約の説明になっている
- [ ] 最後に記事の結論が短く締まっている
