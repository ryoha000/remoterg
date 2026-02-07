# RemoteRG Trace Analysis Scripts

このディレクトリには、RemoteRGのパフォーマンス解析用スクリプトが含まれています。

## analyze_trace.py

`hostd` サービスによって生成された `trace-timestamp.json` (Chrome Tracing 形式) を解析し、フレームごとのレイテンシを計算するスクリプトです。

### 必須要件

- Python 3.x
- `uv` (パッケージマネージャー) または `pandas` ライブラリ

### 使用方法

1. **トレースファイルの生成**
   `hostd` (デスクトップ側) を実行すると、カレントディレクトリに `trace-timestamp.json` が生成されます。
   `hostd` は `tracing-chrome` を使用してトレース情報を出力します。
   
   ```bash
   cd desktop/services
   cargo run --bin hostd
   ```
   
   ⚠️ **注意**: `hostd` のビルド時に `tracing-chrome` の `include_args(true)` が有効になっている必要があります（現在はデフォルトで有効化されています）。これがない場合、`frame_id` が記録されず解析できません。

2. **解析の実行**
   生成されたJSONファイルを引数に指定してスクリプトを実行します。

   ```bash
   cd scripts
   uv run analyze_trace.py ../desktop/services/trace-timestamp.json
   ```

   または `pandas` がインストールされた環境であれば直接 python で実行可能です:
   ```bash
   python analyze_trace.py <path_to_trace__file>
   ```

### 出力内容

スクリプトは以下のメトリクスを計算して表示します：

- **Capture Latency**: キャプチャ開始 (`frame_processing`) からエンコードキュー投入 (`queue_encode_job`) までの時間
- **Encode Latency**: エンコードキュー投入 (`queue_encode_job`) から送信開始 (`write_sample`) までの時間
- **Total Latency**: キャプチャ開始から送信開始までの合計時間

出力例:

```text
==================================================
Trace Analysis Summary (Frames found: 153)
==================================================
       latency_capture_ms  latency_encode_ms  latency_total_ms
count              153.00             153.00            153.00
mean                 2.45               5.12              7.57
std                  0.80               1.20              1.50
min                  1.50               3.20              5.10
50%                  2.30               4.90              7.40
90%                  3.10               6.50              9.20
95%                  3.50               7.10              9.80
99%                  5.20               8.50             11.20
max                  6.00               9.00             12.50

==================================================
Detailed Latency Breakdown (Avg ms)
==================================================
latency_capture_ms    2.45
latency_encode_ms     5.12
latency_total_ms      7.57
dtype: float64
```

### トラブルシューティング

- **"No frame events found with frame_id"**: 
  トレースファイルに `frame_id` 引数が含まれていません。`hostd` 側の `ChromeLayerBuilder` 設定で `.include_args(true)` が有効になっているか確認してください。
- **"File not found"**:
  指定したJSONファイルのパスが正しいか確認してください。
