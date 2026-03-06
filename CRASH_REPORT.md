**調査対象**
- 現象: エミュレータ（`x86_64`）では正常、実機（`arm64`）では映像表示タイミングでクラッシュ
- クラッシュログ: `SIGSEGV` / `IncomingVideoSt` / `libjingle_peerconnection_so.so`
- tombstone: `F:\workspace\remoterg\tombstone_09`

**症状の事実整理**
- クラッシュは映像フレーム受信スレッド（`IncomingVideoSt`）で発生。
- Java/Kotlin例外ではなく、ネイティブ領域での `SIGSEGV`。
- Backtrace は `libjingle_peerconnection_so.so` 側が主で、映像コールバック経路で落ちていた。
- `abiFilters` は既に `arm64-v8a / armeabi-v7a / x86_64` を含んでおり、「ARM向け .so が入っていない」問題ではなかった。

**切り分けでやったこと**
1. ビルド設定確認  
- [`build.gradle.kts`](F:/workspace/remoterg/android/app/build.gradle.kts) で ABI 設定を確認。  
- [`CMakeLists.txt`](F:/workspace/remoterg/android/app/src/main/cpp/CMakeLists.txt) のネイティブコンパイルオプションを確認。

2. ネイティブ経路確認  
- [`LatencyNativeSink.kt`](F:/workspace/remoterg/android/app/src/main/java/moe/ryoha/remoterg/webrtc/LatencyNativeSink.kt) と [`WebRtcManager.kt`](F:/workspace/remoterg/android/app/src/main/java/moe/ryoha/remoterg/webrtc/WebRtcManager.kt) で `VideoTrack.nativeAddSink` 経路を確認。  
- `liblatency_sink.so` が実際に ARM で生成されていることを確認。

3. tombstone 深掘り  
- BuildId を突合し、`libjingle_peerconnection_so.so` と `liblatency_sink.so` が同時にロードされていることを確認。  
- レジスタ値 `x9=0xffffffffa06860ac` と、`liblatency_sink.so` の関数オフセットを照合。

4. 逆アセンブル/ELF検証  
- `libjingle` のクラッシュ周辺命令が、`ldrsw` を使う「relative vtable 前提」の仮想関数ディスパッチであることを確認。  
- 一方で `liblatency_sink.so` 側は当初 absolute 形式 vtable で、ABI前提が噛み合っていなかった。

**根本原因**
- ARM の `libjingle_peerconnection_so.so` は「relative vtable ABI」前提で仮想関数呼び出しを行っていた。
- `LatencyVideoSink` は別DSO（`liblatency_sink.so`）で生成されており、こちらが相対vtable形式でないため、仮想関数ポインタ解釈が壊れた。
- その結果、関数アドレス下位32bitが符号拡張され、不正アドレスへジャンプして `SIGSEGV`。
- エミュレータで再現しなかったのは、`x86_64` 側ではこの組み合わせが問題化しなかったため（推測）。

**実装した修正**
- 変更ファイル: [`CMakeLists.txt`](F:/workspace/remoterg/android/app/src/main/cpp/CMakeLists.txt#L19)
- 変更内容:
- `arm64-v8a` と `armeabi-v7a` のみ `-fexperimental-relative-c++-abi-vtables` を付与。
- `x86_64` は未変更。
- 目的コメントを追加（WebRTC ARMバイナリとの vtable ABI 整合）。

**検証結果**
- `:app:externalNativeBuildDebug` 成功。
- `compile_commands.json` 検証:
- ARM 2種にはフラグあり。
- `x86_64` にはフラグなし。
- `llvm-readelf -r` 検証:
- `LatencyVideoSink` 仮想メソッド/vtable への `R_AARCH64_ABS64` 直指定が消えていることを確認。
- 実機動作:
- 映像表示時クラッシュが解消（ユーザー確認済み）。

**結論**
- 今回の不具合は「ABIフィルタ不足」ではなく、「ARM実機でのみ顕在化する C++ 仮想関数ABIミスマッチ」だった。
- 修正は最小変更（CMakeフラグのABI条件付け）で、公開APIやKotlin/JNI仕様には影響なし。
