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
- Android とネイティブ層をまたいだ観測基盤を設計したい人
- 測定値の意味と成立条件を厳密に扱いたい人

## この記事で前に出したい価値
- WebRTC で遅延を測る難しさは、計算式そのものではなく**フレーム同定**にある
- DataChannel と RTP が別経路である以上、測定系にもアーキテクチャが要る
- Java API だけでは完結しないため、**どこを観測点にするか**まで設計しないといけない

## Abstract
WebRTC ベースの映像配信アプリで E2E レイテンシを測ろうとすると、単に送受信の時刻を記録するだけでは足りない。送信側で観測したフレームと、受信側で表示されたフレームを正しく対応づけ、さらに両者を同じ時間軸で比較する必要があるからだ。

しかし WebRTC では、映像本体は RTP、補助的なメタデータは DataChannel と経路が分かれている。加えて Android の Java API だけでは、フレーム同定に必要な情報を十分に観測できない。そこで本稿では、時刻同期、フレーム同定、ネイティブ観測点、最終計算を分離した計測アーキテクチャを扱う。その全体像と、なぜその責務分担が必要になるのかを説明する。

補足:
- 本稿で扱う E2E は、成立条件を満たして対応づけできたフレームに対する厳密な測定値であり、すべてのフレームに対する近似値ではない

## 1. 何を測りたかったのか

### 1.1 測りたいもの
- 測りたいのは、送信側でキャプチャされたフレームが Android クライアントで表示されるまでの時間
- 起点は送信側のキャプチャ時刻
- 終点は受信側の描画時刻
- つまり知りたいのは、あるフレームが「取られてから表示されるまで」の時間

### 1.2 でも難しいのは式ではなく対応づけ
- 数式だけ見ると単純だが、実際には「その描画時刻が、どのキャプチャ時刻に対応するのか」が分からない
- 計測を成立させるには、次の 2 条件が必要になる
  - 送信側と受信側で**同じフレーム**を見つけられること
  - その 2 つの時刻を**同じ時間軸**で比較できること

## 2. なぜ素朴には測れないのか

### 2.1 素朴な方法はどこで壊れるか
- たとえばフレームごとのメタデータを DataChannel で送り、受信側で近い時刻に届いた映像フレームと対応づける方法は、一見すると十分に見える
- しかし実際には、その「近い時刻の映像フレーム」が本当に同じフレームかを保証できない
- つまり「表示時刻 - キャプチャ時刻」という式は簡単でも、その前段の観測系は簡単ではない

### 2.2 DataChannel と RTP は別経路
- フレームに付随するメタデータは DataChannel で届く
- 映像フレームは RTP で届く
- この 2 つは順序、遅延、ドロップの仕方が一致しない
- だから DataChannel 単独では、主計測値に必要なフレーム同定を保証できない

### 2.3 送信側と Android の時計は別
- キャプチャ時刻は送信側の monotonic
- 描画時刻は Android の monotonic
- そのままでは引き算できない
- だから clock offset を別途推定する必要がある

### 2.4 Java API からは必要な capture 情報が見えない
- 欲しいのは、RTP 側に乗ってきた capture 時刻
- しかし Java の `VideoFrame` から見える情報だけでは足りない
- `packet_infos` や `absolute_capture_time` は C++ 側にはあっても Java 側では直接使えない
- だから観測点を Java より下に置く必要がある

## 3. 問題を4つの責務に分解する

### 3.1 1つの仕組みで全部解こうとすると破綻する
- この問題を単一のキーや単一の経路で解こうとすると、どこかで曖昧さが入る
- そこで計測を次の 4 つに分ける
  - 時計を揃える
  - 同じフレームを見つける
  - Java では見えない情報を観測する
  - 同じ時間軸で最後に引き算する

### 3.2 この分解が意味すること
- 時刻同期とフレーム同定は別問題として扱う
- 観測点の設計と最終計算も別問題として扱う
- この責務分解は特定アプリ固有というより、複数経路と複数時間軸を持つ WebRTC 系の計測全般で再利用しやすい

## 4. 採用した計測アーキテクチャ

### 4.1 全体像
- 送信側は 2 系統で計測情報を送る
  - DataChannel: 時計合わせ用メッセージとフレームメタデータ
  - RTP: キャプチャ時刻を運ぶ拡張情報
- Android は 2 つの観測点を持つ
  - DataChannel 受信点
  - C++ `VideoFrame` を読む独自の native ビデオシンク
- フレーム同定は 2 段で行う
  - 第1段: 絶対時刻ベースの照合キーで、DataChannel 側メタデータと native callback を結ぶ
  - 第2段: デコーダ側フレーム時刻で、native callback と render を結ぶ
- 最終計算は monotonic ベース
  - 送信側キャプチャ時刻を client 側時間軸へ変換する
  - そのうえで描画時刻との差をとる

### 4.2 全体設計の読み方
- このアーキテクチャのポイントは、1つの仕組みで全部を解かないこと
- DataChannel は時計合わせと送信側メタデータ伝送
- RTP extension は映像本流側のフレーム同定
- JNI sink は lower layer の観測点
- monotonic 計算は最終値の意味づけ

## 5. 4つの責務をどう実装したか

### 5.1 時刻同期: DataChannel
- 入力:
  - 時計合わせ用の往復メッセージ
- 出力:
  - 送信側と client 側の時計差推定値
- 保証:
  - 送信側 monotonic と Android monotonic を対応づけるための基準を作る
- 非保証:
  - これだけではフレーム同定はできない
- 実装では:
  - `sync_req` / `sync_res`
  - `offsetMonoMs`
- 参照実装:
  - `desktop/services/core/src/lib.rs`
  - `desktop/services/webrtc/src/connection.rs`
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyMonotonicMath.kt`
- 一般化:
  - 補助経路で時計同期を行い、本流とは別に時間軸対応を作る分離は、他の WebRTC 系でも応用しやすい

### 5.2 フレーム同定のための送信側メタデータ
- 入力:
  - 送信側のキャプチャ時刻
  - エンコード投入時刻
  - エンコード完了時刻
  - 送信時刻
- 出力:
  - DataChannel 上のフレームメタデータ
  - 第1段照合に使う絶対時刻ベースのキー
- 保証:
  - 送信側起点のキャプチャ時刻を受信側まで運ぶ
  - JNI 経路との結合キーを与える
- 非保証:
  - これ単独では「その映像フレーム」との 1:1 対応は保証しない
- 実装では:
  - `frame_sample`
  - `t_cap`, `t_enc_in`, `t_enc_out`, `t_send`
  - `capture_unix_ms`
- 参照実装:
  - `desktop/services/core/src/lib.rs`
  - `desktop/services/webrtc/src/connection.rs`
- 一般化:
  - 本流と補助経路の情報を分けて運び、後段で結合する構成は計測系を安定させやすい

### 5.3 映像本流からのフレーム同定
- 入力:
  - 送信フレームのキャプチャ時刻
- 出力:
  - RTP extension 上のキャプチャ時刻情報
- 保証:
  - DataChannel とは別に、映像本流からフレーム由来の時刻情報を得られる
- 非保証:
  - Android Java API だけでは直接観測できない
- 実装では:
  - `abs-capture-time`
- 参照実装:
  - `desktop/services/video-stream/src/track_writer.rs`
  - `desktop/services/webrtc/src/connection.rs`
- 一般化:
  - 補助経路ではなく映像本流からフレーム由来の情報を取ることが、E2E 計測の信頼性を上げる

### 5.4 観測点: ネイティブ側ビデオシンク
- 入力:
  - C++ `VideoFrame`
- 出力:
  - 絶対時刻ベースの照合キー
  - デコーダ側フレーム時刻
- 保証:
  - Java API では見えない `packet_infos.absolute_capture_time` を観測できる
- 非保証:
  - これ単体では render との対応も E2E も確定しない
- 実装では:
  - `captureUnixMs`
  - `timestampUs`
- 参照実装:
  - `android/app/src/main/cpp/latency_sink.cpp`
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt`
- 一般化:
  - Java API で必要情報が取れない場合、観測点を lower layer に置くのは再利用しやすい設計原則になる

### 5.5 フレーム同定: 2段突合
- 第1段:
  - 入力: DataChannel 側メタデータの照合キーと、native callback 側の照合キー
  - 出力: 送信側キャプチャ時刻とデコーダ側フレーム時刻の結合
  - 保証: 送信側のフレーム情報と native 側のフレーム情報を結ぶ
- 第2段:
  - 入力: デコーダ側フレーム時刻と render 側のフレーム時刻
  - 出力: client 時間軸にそろえたキャプチャ時刻と描画時刻の組
  - 保証: 実際に render されたフレームと対応づける
- 非保証:
  - 一致しないフレームは無理に計算しない
- 実装では:
  - 第1段キー: `capture_unix_ms` / `captureUnixMs`
  - 第2段キー: `timestampUs`
- 参照実装:
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/FrameNativeMatchStore.kt`
  - `android/app/src/main/java/moe/ryoha/remoterg/webrtc/WebRtcManager.kt`
- 一般化:
  - 異なる経路の情報を1つのキーで無理に合わせず、段階的に対応づけるほうが安定する

### 5.6 最終計算: monotonic に揃えて E2E を出す
- 入力:
  - 送信側キャプチャ時刻
  - 時計差推定値
  - client 側描画時刻
- 出力:
  - E2E レイテンシ
- 保証:
  - 最終値の時間軸を monotonic に揃えられる
- 非保証:
  - display の実発光時刻までは表していない
- 実装では:
  - `tCapSenderMonoMs`
  - `offsetMonoMs`
  - `tRenderClientMonoMs`
- 一般化:
  - 同定に便利な絶対時刻と、最終計算に向く monotonic を分離するのは、時間軸が混在する計測で有効

## 6. この設計で何が測れて、何が測れないか

### 6.1 測れているもの
- 送信側の capture から Android の render callback までの E2E
- `frame_sample` が持つ encode 周辺の補助指標
- 成立条件を満たしたフレームについての厳密な E2E

### 6.2 測っていないもの
- 実ディスプレイの発光時刻そのもの
- 成立条件を満たさなかったフレームの E2E
- `abs-capture-time` 欠落や突合失敗を含む全フレーム一律の値

### 6.3 重要な姿勢
- 「すべてのフレームをざっくり測る」より
- 「成立条件を満たしたフレームだけを厳密に測る」を優先する
- 計測記事としては、ここを明示したほうが信頼される

## 7. 実装上の注意点

### 7.1 absolute time はキー用途に限定する
- 絶対時刻ベースのキーはフレーム同定には便利
- ただし最終 E2E を絶対時刻同士の差で持つと基準がぶれやすい
- だから absolute time は橋渡しに限定し、最終値は monotonic で計算する
- 実装では:
  - `capture_unix_ms`

### 7.2 デコーダ側フレーム時刻は第2段突合専用にする
- Java から見えるデコーダ側フレーム時刻は capture 時刻そのものではない
- render 側との結合には使える
- ただし送信側起点の時刻とみなしてはいけない
- 実装では:
  - `timestampNs` / `timestampUs`

### 7.3 Java デコーダラップを主経路にしない
- `WrappedNativeVideoDecoder` のように、Java ラップを主経路にできないケースがある
- だからデコーダ内部ではなく、デコード後 `VideoFrame` を観測する構成にしている

### 7.4 JNI sink は ABI 整合まで含めて責任を持つ
- C++ 側の観測点を使う以上、ABI 整合も設計責任に入る
- 特に ARM 実機では relative vtable ABI への注意が必要だった
- ただし本文では主軸にしすぎず、実装コラム寄りに扱う
- 参照実装:
  - `android/app/src/main/cpp/CMakeLists.txt`
  - `android/app/build.gradle.kts`

## 8. まとめ

### 8.1 記事の結論候補
- WebRTC の E2E レイテンシ計測で最初に解くべき問題は、計算式ではなくフレーム同定である
- DataChannel と RTP が別経路である以上、測定系にも役割分担が必要になる
- その制約を分解すると
  - 時刻同期は DataChannel
  - フレーム同定は RTP `abs-capture-time`
  - 観測点は JNI `VideoSink`
  - 最終計算は monotonic
  という構成が自然に導かれる

### 8.2 記事の価値
- 「ある実装ではこうした」だけでなく
- 「WebRTC のレイテンシ計測では、なぜその責務分担が必要になるのか」を説明できる

## 9. 記事内で入れたい図
- 図1: 採用アーキテクチャ全体図
- 図2: 2 段突合のシーケンス図
- 図3: 時間軸の整理
  - sender monotonic
  - client monotonic
  - absolute time
- 図4: Java API と C++ `VideoFrame` の見える情報の差
- 図5: JNI sink を入れたときの観測点と責務範囲

## 10. 章ごとに差し込むコード参照候補
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
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/FrameNativeMatchStore.kt`
  - 第1段突合
  - 第2段突合
- `android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt`
  - sink attach / detach
- `android/app/src/main/cpp/latency_sink.cpp`
  - `ExtractCaptureTimeFromFrame`
  - JNI callback
- `android/app/src/main/cpp/CMakeLists.txt`
  - ARM ABI 対応

## 11. 仕上げ時のチェックリスト
- [ ] 冒頭で「何がそんなに難しいのか」が人間語で伝わる
- [ ] 素朴な方法がどこで壊れるかを序盤で示している
- [ ] 用語の交通整理が早い段階で済んでいる
- [ ] 責務分解を先に宣言してから実装説明に入っている
- [ ] 全体図を早めに出している
- [ ] 章立てが `問題設定 -> 素朴には測れない理由 -> 責務分解 -> 全体設計 -> 各責務の実装 -> 測定範囲 -> 実装上の注意 -> まとめ` になっている
- [ ] 送信側メタデータ、RTP 側のキャプチャ時刻、ネイティブ観測点、2 段突合、monotonic 計算の役割が分離して説明されている
- [ ] 失敗談が日記ではなく設計制約の説明になっている
- [ ] 「何が測れて何が測れないか」が明示されている
- [ ] 固有実装の説明に、一般化できる一文を適宜添えている
