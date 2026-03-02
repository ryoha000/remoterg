# キャラクター識別パイプライン設計

## 背景・目的

現在の実装では、VNDBからダウンロードしたキャラクター立ち絵画像をbase64エンコードし、VLLMのマルチモーダル入力として最大16枚を一括送信している（`tagger` の `analyze_with_references`）。しかし、この方式では以下の問題がある：

- VLLMに画像同士の視覚的マッチングを任せているため**精度が非常に低い**
- 大量の画像をbase64で送信するため**リクエストサイズが巨大**になり、推論も遅い
- VLLMはテキスト生成に最適化されており、画像間の類似度判定は得意ではない

そこで、以下のパイプラインに分離することで精度を改善する：

```
スクリーンショット → 顔検出・crop → Image Embedding → 参考画像との類似度比較 → キャラ名特定
```

VLLMはキャラクター識別以外のタスク（シーン情報、セリフ読み取りなど）に引き続き使用する。

### 採用技術

- **顔検出**: YOLOv8 Anime Face Detection（Precision 0.957, Recall 0.924）
- **Image Embedding**: DINOv2 ViT-S/14（細かな視覚的差異の識別に強い、88.5MB）
- **推論ランタイム**: ONNX Runtime（`ort` crate、CPU/GPU切替対応）

## アーキテクチャ概要

```
┌─────────────────────────────────────────────────────────┐
│                    スクリーンショット                      │
└─────────────┬───────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────┐
│  Step 1: 顔検出 (YOLOv8 Anime) │  ← スクリーンショットから顔領域をcrop
└─────────────┬───────────────────┘
              │  顔画像群 (bounding box + crop)
              ▼
┌──────────────────────────────┐     ┌───────────────────────────────┐
│  Step 2: Embedding生成       │     │  参考画像DB (VNDB立ち絵)       │
│  (DINOv2 ViT-S/14)          │     │  → 顔cropしてEmbedding事前計算 │
└─────────────┬────────────────┘     └───────────────┬───────────────┘
              │                                       │
              ▼                                       ▼
┌──────────────────────────────────────────────────────────┐
│  Step 3: コサイン類似度比較 → 閾値判定 → キャラ名特定     │
└─────────────┬────────────────────────────────────────────┘
              │  識別結果 (キャラ名 + 信頼度 + 位置)
              ▼
┌──────────────────────────────────────────────────────────┐
│  Step 4: VLLMに渡す情報を構築                             │
│  （キャラ名のテキスト情報のみ。画像はスクリーンショット1枚）│
└──────────────────────────────────────────────────────────┘
```

## クレート構成

新規クレート `character-identifier` を作成する。

```
desktop/services/character-identifier/
├── Cargo.toml
├── src/
│   ├── lib.rs           # 公開API（CharacterIdentifier）
│   ├── face_detector.rs # YOLOv8による顔検出
│   ├── embedder.rs      # DINOv2によるEmbedding生成
│   └── matcher.rs       # コサイン類似度比較・キャラ特定
└── models/              # ONNXモデルファイル配置場所（.gitignore対象）
```

### 依存関係

```
character-identifier
├── ort (ONNX Runtime)      # 推論エンジン
├── image                   # 画像の前処理（crop, resize）
├── ndarray                 # テンソル操作
├── anyhow / tracing        # エラーハンドリング・ログ
└── serde / bincode         # Embeddingキャッシュのシリアライズ
```

**重要**: `character-identifier` は他のサービスクレートに直接依存しない。`input` クレートから呼び出す。

### ランタイムCPU/GPU切替

CUDA Execution Provider (EP) は常にビルドに含め、**起動後にAndroidアプリからCPU/GPUを切り替えられる**ようにする。

```toml
[dependencies]
ort = { version = "2.0.0-rc", features = ["cuda"] }  # 常にCUDAを含む
```

```rust
/// 実行デバイス
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    Cpu,
    Gpu,
}

impl CharacterIdentifier {
    /// デバイスを切り替える（セッションを再作成）
    pub fn set_device(&mut self, device: DeviceType) -> Result<()>;
}
```

- `DeviceType::Gpu` 選択時: `CUDAExecutionProvider` を使用してセッションを作成
- `DeviceType::Cpu` 選択時: `CPUExecutionProvider` のみでセッションを作成
- 切り替え時はONNXセッションを再作成するため、一瞬のラグが発生する（数百ms程度）
- 初期値は `DeviceType::Cpu`（CUDA未インストール環境でもエラーにならない）

## 公開API

```rust
/// キャラクター識別サービス
pub struct CharacterIdentifier {
    face_detector: FaceDetector,
    embedder: Embedder,
    /// (キャラ名, Embeddingベクトル) のキャッシュ
    reference_embeddings: Vec<(String, Vec<f32>)>,
}

/// 識別結果
pub struct IdentifiedCharacter {
    /// キャラクター名
    pub name: String,
    /// コサイン類似度 (0.0 - 1.0)
    pub confidence: f32,
    /// 左からの位置インデックス (0始まり、X座標の昇順でソート)
    pub position_index: usize,
    /// バウンディングボックス (x, y, w, h) 正規化座標
    pub bbox: (f32, f32, f32, f32),
}

impl CharacterIdentifier {
    /// ONNXモデルを読み込んで初期化
    /// models_dir: YOLOv8とDINOv2のONNXモデルが配置されたディレクトリ
    pub fn new(models_dir: &Path) -> Result<Self>;

    /// VNDB参考画像を登録（顔crop + Embedding計算 + キャッシュ）
    /// 既にキャッシュがあればディスクから読み込む
    pub async fn register_references(
        &mut self,
        characters: &[(String, Vec<u8>)],  // (キャラ名, 画像データ)
        cache_dir: &Path,                   // Embeddingキャッシュ保存先
        vndb_id: &str,
    ) -> Result<()>;

    /// スクリーンショットからキャラクターを識別
    pub fn identify(&self, screenshot: &[u8]) -> Result<Vec<IdentifiedCharacter>>;
}
```

## 処理詳細

### Step 1: 顔検出 (YOLOv8 Anime Face)

1. スクリーンショットをYOLOv8の入力サイズ（640x640）にリサイズ + パディング
2. ピクセルを `[0.0, 1.0]` に正規化、CHW形式に変換
3. ONNX Runtimeで推論
4. 出力テンソルからバウンディングボックスを抽出
5. **NMS（Non-Maximum Suppression）** で重複除去
6. 信頼度閾値（デフォルト 0.5）以上の検出のみ採用
7. 元画像座標に変換してcrop

### Step 2: Embedding生成 (DINOv2 ViT-S/14)

1. crop画像を224x224にリサイズ
2. ImageNet正規化（mean=[0.485, 0.456, 0.406], std=[0.229, 0.224, 0.225]）
3. CHW形式、float32に変換
4. ONNX Runtimeで推論 → 384次元のEmbeddingベクトル
5. L2正規化（コサイン類似度計算のため）

### Step 3: マッチング

1. 検出された各顔のEmbeddingと、全参考EmbeddingのCosine類似度を計算
2. 各顔に対して最も類似度が高い参考キャラを割り当て
3. 類似度が閾値（デフォルト 0.6）未満の場合は「不明」とする
4. バウンディングボックスのX座標中心で左から昇順にソートし、`position_index` を `0, 1, 2, ...` と振る

### Embeddingキャッシュ

```
characters_dir/{vndb_id}/
├── embeddings.bin          # bincode形式: Vec<(String, Vec<f32>)>
├── キャラ名1.jpg           # 元画像（既存）
├── キャラ名2.jpg
└── ...
```

- `embeddings.bin` が存在すれば画像から再計算せずにキャッシュを読み込む
- VNDB画像のダウンロード後、初回のみ顔検出 + Embedding計算を実行
- キャッシュ無効化: `embeddings.bin` を削除するか、vndb_idが変わった場合

## input クレートの変更点

### 現在の `analyze_with_references` 呼び出しを置換

**Before:**
```rust
// キャラ画像がある場合は analyze_with_references を使用
let mut rx = if !char_images.is_empty() {
    tagger_service.analyze_with_references(&image_data, &char_images, &prompt).await?
} else {
    tagger_service.analyze_screenshot_stream(&image_data, &prompt).await?
};
```

**After:**
```rust
// パイプラインでキャラクター識別
let identified = if let Some(ref identifier) = character_identifier {
    identifier.identify(&image_data)?
} else {
    Vec::new()
};

// 識別結果をプロンプトに注入
let prompt = build_prompt_with_characters(&identified);

// VLLMにはスクリーンショット1枚のみ送信（参考画像の送信不要）
let mut rx = tagger_service.analyze_screenshot_stream(&image_data, &prompt).await?;
```

### プロンプト注入の形式

```
以下のキャラクターがスクリーンショットに表示されています（左から順）：
- 位置0: 「キャラ名A」(信頼度: 0.85)
- 位置1: 「キャラ名B」(信頼度: 0.92)

この情報を参考にして、以下のスキーマに従って解析してください。
（以下、既存のJSON Schemaプロンプト）
```

## ONNXモデルの取得

### YOLOv8 Anime Face

```bash
# HuggingFace からダウンロード後、ONNXにエクスポート
pip install ultralytics
yolo export model=yolov8_animeface.pt format=onnx
```

もしくは直接ONNX形式のモデルをHuggingFaceから取得。

### DINOv2 ViT-S/14

```python
# PyTorchからONNXにエクスポート
import torch
model = torch.hub.load('facebookresearch/dinov2', 'dinov2_vits14')
dummy = torch.randn(1, 3, 224, 224)
torch.onnx.export(model, dummy, "dinov2_vits14.onnx", opset_version=14)
```

もしくは HuggingFace のONNX版を使用: `onnx-community/dinov2-small-onnx`

## リソース見積もり

RTX 4070Ti SUPER (VRAM 16GB) でVLLMと共存する想定:

| コンポーネント | GPU VRAM | CPU RAM |
|--------------|---------|---------|
| YOLOv8 Anime Face (FP16) | ~300MB | ~500MB |
| DINOv2 ViT-S/14 (FP16) | ~200MB | ~350MB |
| 合計 | **~500MB** | ~850MB |

> [!TIP]
> GPU合計約500MBと非常に軽量。VLLMが10-12GB使用しても余裕で共存可能。CPU動作時はVRAM消費ゼロ。

## テスト

### ユニットテスト

各モジュールごとにユニットテストを実装する。

#### `face_detector.rs`

| テスト名 | 内容 |
|---------|------|
| `test_preprocess_image` | 入力画像が640x640にリサイズ+パディングされ、CHW float32テンソルに変換されることを確認 |
| `test_nms` | NMS処理で重複するバウンディングボックスが正しく除去されることを確認 |
| `test_postprocess_output` | YOLOの生出力テンソルからバウンディングボックスが正しく抽出されることを確認 |

#### `embedder.rs`

| テスト名 | 内容 |
|---------|------|
| `test_preprocess_normalize` | 224x224リサイズ + ImageNet正規化の数値が正しいことを確認 |
| `test_l2_normalize` | Embeddingベクトルのノルムが1.0になることを確認 |

#### `matcher.rs`

| テスト名 | 内容 |
|---------|------|
| `test_cosine_similarity` | 同一ベクトル→1.0、直交ベクトル→0.0 を確認 |
| `test_match_threshold` | 閾値以下の類似度の場合にマッチしないことを確認 |
| `test_position_index_ordering` | X座標が小さい順に `position_index` が振られることを確認 |
| `test_best_match_selection` | 複数の参考画像から最も類似度が高いものが選択されることを確認 |

### 結合テスト（ONNXモデル必要）

モデルファイルが必要なテストは `#[ignore]` を付与し、通常の `cargo test` では実行しない。

```bash
# モデルファイルを配置した上で実行
cargo test -p character-identifier -- --ignored
```

| テスト名 | 内容 |
|---------|------|
| `test_detect_faces_in_screenshot` | 実際のアニメスクリーンショットで顔検出が動作し、1つ以上のbboxが返ることを確認 |
| `test_embedding_output_shape` | DINOv2の出力が384次元のベクトルであることを確認 |
| `test_same_character_high_similarity` | 同一キャラの異なる画像から生成したEmbeddingのコサイン類似度が高い（>0.7）ことを確認 |
| `test_different_character_low_similarity` | 異なるキャラの画像から生成したEmbeddingのコサイン類似度が低い（<0.5）ことを確認 |
| `test_end_to_end_identify` | スクリーンショット + 参考画像のフルパイプラインで正しいキャラ名が返ることを確認 |
| `test_device_switching` | `set_device(Cpu)` → `set_device(Gpu)` の切替後も正常に推論できることを確認 |

### テスト用データ

結合テストには以下のテストデータを `desktop/services/character-identifier/testdata/` に配置する（.gitignore対象）：

- `screenshot_sample.jpg` — キャラクターが映ったVNスクリーンショット
- `reference_char_a.jpg` — キャラAの立ち絵（VNDB画像）
- `reference_char_b.jpg` — キャラBの立ち絵

### 実行方法

```bash
# ユニットテスト（モデル不要）
cargo test -p character-identifier

# 結合テスト（モデル + テストデータ必要）
cargo test -p character-identifier -- --ignored
```
