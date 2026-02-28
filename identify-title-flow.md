# ゲームタイトル推定パイプライン

## 目的

`window-info` から取得できる `process_path`（例: `G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe`）を入力として、VNDB のゲームタイトルを推定する。

## 入力

```
ScreenshotMetadataPayload {
  process_path: "G:\\game\\Heliodor\\流星ワールドアクターGB\\WorldActorGB.exe"
  window_title: "流星ワールドアクター Gaslight Bullet"  // 補助的に使用
}
```

## マッチ対象の多様性

VNDB 上の各ゲームは以下の複数の名前パターンを持つ:

| 種類 | 例 |
|---|---|
| Brand 名 (name) | `Heliodor`, `フリクル`, `minori` |
| Brand Latin | `FuriKuru`, `Lump of Sugar` |
| Brand Alias | `みのり 中二社`, `角砂糖 方糖社` |
| Title (各言語・Official/Unofficial) | `流星ワールドアクター Gaslight Bullet`, `Trinoline`, `トリノライン` |
| Title Latin | `Ryuusei World Actor: Gaslight Bullet`, `12 no Tsuki no Eve` |
| VN Alias | `いつ空`, `TRIノLINE`, `12mooneve` |

パスの各セグメント（ディレクトリ名・ファイル名）はこれらの**いずれか**に部分一致する可能性がある。

## パイプライン概要

```
process_path
    │
    ▼
┌─────────────────────────────┐
│ Step 1: パスセグメント抽出   │
│ パスをディレクトリ名・ファイル │
│ 名に分解し候補トークンを生成 │
└─────────────┬───────────────┘
              │
              ▼
┌─────────────────────────────┐
│ Step 2: 前処理・正規化       │
│ 拡張子除去、記号分離、       │
│ CamelCase/snake_case 分割   │
└─────────────┬───────────────┘
              │
              ▼
┌─────────────────────────────┐
│ Step 3: 辞書マッチング       │
│ VNDB データの全名前パターン  │
│ と文字列マッチング           │
└─────────────┬───────────────┘
              │
              ▼
┌─────────────────────────────┐
│ Step 4: スコアリング・選定   │
│ 複数ステージの結果を統合し   │
│ 最終候補を 1 件に絞り込む    │
└─────────────────────────────┘
```

---

## Step 1: パスセグメント抽出

`process_path` をパス区切り文字 (`\` / `/`) で分割し、**意味のあるセグメント**を抽出する。

```
入力: G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe

分割結果:
  [0] G:           → ドライブレター → 除外
  [1] game         → ジェネリック   → 除外
  [2] Heliodor     → ★ 候補
  [3] 流星ワールドアクターGB → ★ 候補
  [4] WorldActorGB.exe       → ★ 候補 (.exe 除去)
```

### 除外ルール

以下のセグメントはジェネリックとして除外:

- ドライブレター (`C:`, `G:` など)
- よくあるジェネリック名: `game`, `games`, `Program Files`, `Program Files (x86)`, `eroge`, `visual novel`, `VN`, `Users`, `Desktop`, `Download`, `bin`, `lib`, `data` など

### ファイル名ブラックリスト（拡張子除去後に適用）

ファイル名セグメント（パスの最後の要素）が以下のいずれかに一致する場合、そのセグメントを**候補から除外**する:

- **エンジン名**: `SiglusEngine`, `BGI`, `Ethornell`, `Kirikiri`, `Rio`, `AlphaROMdiS`, `CatSystem2`, `YU-RIS`, `Majiro`, `AdvHD`, `QLIE` など
- **インストーラー/ユーティリティ**: `setup`, `install`, `uninst`, `uninstall`, `patch`, `update`, `launcher`, `config`, `readme`, `main`

> [!TIP]
> エンジン名リストは VNDB の `engines` テーブルから取得可能（`engines.name`）。静的リストを手動管理するのではなく、VNDB データから動的に構築するのが望ましい。

### 生成されるセグメント一覧

```
segments = ["Heliodor", "流星ワールドアクターGB", "WorldActorGB"]
```

---

## Step 2: 前処理・正規化

### トークンの定義

**トークン = Step 3 でマッチング対象となる 1 つの検索文字列**。
個別の単語ではなく、常に**フレーズ（複数語のスペース結合）** として保持する。
`FuriKuru_Game` から `Furi`, `Kuru`, `Game` のような単語単位のトークンは**生成しない**。

### 変換ルール

Step 1 で得た各セグメントに対して、以下のルールを**順番に**適用し、複数のトークンを生成する。

#### ルール A: 拡張子除去
- `.exe`, `.bin`, `.log`, `.bat` などの既知拡張子を除去
- 元の拡張子付きセグメントは保持しない（拡張子はノイズのため）

#### ルール B: 区切り文字展開
- `_`, `-` で分割し、スペースで再結合したバリエーションを追加
- 区切り文字以降を「サブタイトル」と見なし、区切り文字より**前だけ**を抽出したバリエーションを追加（例: `サクラノ刻－櫻の森の下を歩む－` → `サクラノ刻`）
- 元のセグメント（区切り文字付き）も保持する

#### ルール C: CamelCase 展開
- 大文字の境界（`[a-z][A-Z]` や `[A-Z][A-Z][a-z]`）でスペースを挿入したバリエーションを追加
- 元のセグメントも保持する

#### ルール D: 文字種境界展開
- 日本語⇔ASCII、英字⇔数字 の切り替わりでスペースを挿入したバリエーションを追加
- 元のセグメントも保持する

#### ルール E: 略称・頭文字結合展開
- 複数の単語（スペース区切りやCamelCase区切り）で構成されるセグメントに対して、頭文字を結合したパターンを追加（例: `WHITE ALBUM 2` → `wa2`, `World Actor GB` → `wagb`）
- 1つ目の単語＋以降の頭文字を結合したパターンを追加（例: `流星ワールドアクター Gaslight Bullet` → `流星ワールドアクターgb`）
- さらに、すべてのスペースを除去した密着パターンを追加（例: `Sakura no Toki` → `sakuranotoki`）

#### ルール F: ジェネリックサブワード除去
- 上記展開結果に `Game`, `App`, `Launcher` などのジェネリック語が含まれている場合、それを除去した短縮版を追加

#### ルール G: 重複排除
- 全ルール適用後、同一の文字列は 1 つにまとめる

> [!NOTE]
> **末尾数字の除去は行わない。** `WHITE ALBUM 2` のように数字がタイトルの一部であるケースを誤って壊すため。
> `FuriKuru01` のようなケースはルール D（数字/英字境界）で `FuriKuru 01` が生成され、包含マッチで十分カバーできる。

### 全ケースの完全な変換結果

以下に、Step 1 の各セグメントに対してどのルールが適用され、最終的にどのようなトークン群が生成されるかを示す。

---

#### Case 1: `G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe`

Step 1 のセグメント: `Heliodor`, `流星ワールドアクターGB`, `WorldActorGB.exe`

| セグメント | 適用ルール | 生成されるトークン |
|---|---|---|
| `Heliodor` | (変換なし) | `Heliodor` |
| `流星ワールドアクターGB` | D: 境界 | `流星ワールドアクターGB`, `流星ワールドアクター GB` |
| `流星ワールドアクター GB` | E: 頭文字结合 | `流星ワールドアクターgb` |
| `WorldActorGB.exe` | A: 拡張子除去 | `WorldActorGB` |
| `WorldActorGB` | C: CamelCase | `World Actor GB` |
| `World Actor GB` | E: 頭文字结合 | `wagb`, `Worldgb` |

**最終トークンリスト:**
```
["Heliodor", "流星ワールドアクターGB", "流星ワールドアクター GB", "流星ワールドアクターgb", "WorldActorGB", "World Actor GB", "wagb", "Worldgb"]
```

---

#### Case 2: `G:\game\いつか、届く、あの空に。\main.bin`

Step 1 のセグメント: `いつか、届く、あの空に。`, `main.bin`

| セグメント | 適用ルール | 生成されるトークン |
|---|---|---|
| `いつか、届く、あの空に。` | (変換なし) | `いつか、届く、あの空に。` |
| `main.bin` | A: 拡張子除去 | `main` |

**最終トークンリスト:**
```
["いつか、届く、あの空に。", "main"]
```

※ `main` はジェネリック的だが、Step 2 ではあくまで文字列変換のみ行う。マッチング時に低スコアとなるため自然に淘汰される。

---

#### Case 3: `G:\game\FuriKuru_Game\諦観のイヴ・ベセル\FuriKuru01.exe`

Step 1 のセグメント: `FuriKuru_Game`, `諦観のイヴ・ベセル`, `FuriKuru01.exe`

| セグメント | 適用ルール | 生成されるトークン |
|---|---|---|
| `FuriKuru_Game` | B: 区切り文字展開 | `FuriKuru_Game`, `FuriKuru Game` |
| `FuriKuru Game` | E: ジェネリック除去 | `FuriKuru` |
| `FuriKuru` | C: CamelCase | `Furi Kuru` |
| `諦観のイヴ・ベセル` | (変換なし) | `諦観のイヴ・ベセル` |
| `FuriKuru01.exe` | A: 拡張子除去 | `FuriKuru01` |
| `FuriKuru01` | D: 数字/英字境界 | `FuriKuru 01` |
| `FuriKuru01` | C: CamelCase | `Furi Kuru01` |
| `Furi Kuru01` | D: 数字/英字境界 | `Furi Kuru 01` |

**最終トークンリスト:**
```
["FuriKuru_Game", "FuriKuru Game", "FuriKuru", "Furi Kuru", "諦観のイヴ・ベセル", "FuriKuru01", "FuriKuru 01", "Furi Kuru01", "Furi Kuru 01"]
```

ポイント: `Furi`, `Kuru`, `Game` が**個別のトークンになることはない**。常にフレーズ単位。

---

#### Case 4: `G:\game\trinoline\trinoline.exe`

Step 1 のセグメント: `trinoline`, `trinoline.exe`

| セグメント | 適用ルール | 生成されるトークン |
|---|---|---|
| `trinoline` | (変換なし) | `trinoline` |
| `trinoline.exe` | A: 拡張子除去 | `trinoline` (重複→除去) |

**最終トークンリスト:**
```
["trinoline"]
```

---

#### Case 5: `F:\game\12eve\12eve.exe`

Step 1 のセグメント: `12eve`, `12eve.exe`

| セグメント | 適用ルール | 生成されるトークン |
|---|---|---|
| `12eve` | D: 数字/英字境界 | `12eve`, `12 eve` |
| `12eve.exe` | A: 拡張子除去 | `12eve` (重複→除去) |

**最終トークンリスト:**
```
["12eve", "12 eve"]
```

#### Case 6: `F:\game\サクラノ刻\sakuranotoki.exe`

Step 1 のセグメント: `サクラノ刻`, `sakuranotoki.exe`

| セグメント | 適用ルール | 生成されるトークン |
|---|---|---|
| `サクラノ刻` | (変換なし) | `サクラノ刻` |
| `sakuranotoki.exe` | A: 拡張子除去 | `sakuranotoki` |

**最終トークンリスト:**
```
["サクラノ刻", "sakuranotoki"]
```

---

## Step 3: 辞書マッチング

### 事前準備: VNDB 名前辞書の構築

VNDB のデータから、各ゲーム (`vn.id`) に紐づく**全ての名前文字列**を正規化して辞書を構築する。

```
辞書エントリの構造:
  normalized_name → [(game_id, match_type, original_name)]
```

#### 辞書に登録する名前の一覧

各 VN について、以下の名前を全て登録する:

| ソース | フィールド | match_type | 例 |
|---|---|---|---|
| `vn_titles` | `title` | `title` | `流星ワールドアクター Gaslight Bullet` |
| `vn_titles` | `latin` | `title_latin` | `Ryuusei World Actor: Gaslight Bullet` |
| `vn` | `alias` | `alias` | `いつ空`, `TRIノLINE` |
| `producers` | `name` | `brand` | `Heliodor`, `フリクル` |
| `producers` | `latin` | `brand_latin` | `FuriKuru`, `Lump of Sugar` |
| `producers` | `alias` | `brand_alias` | `みのり`, `中二社`, `角砂糖` |

> [!NOTE]
> `vn.alias` と `producers.alias` は、1つのフィールドに複数のエイリアスが **改行文字 (`\n`)** で区切られて格納されている。
> 辞書登録時にはこれを分割して**個別のエントリ**として登録する。
>
> ```
> vn.alias の生データ: "いつ空\nItsusora"
>   → 辞書エントリ 1: "いつ空"    → (v23, alias)
>   → 辞書エントリ 2: "Itsusora"  → (v23, alias)
>
> producers.alias の生データ: "みのり\n中二社"
>   → 辞書エントリ 1: "みのり"    → (v19644, brand_alias)
>   → 辞書エントリ 2: "中二社"    → (v19644, brand_alias)
> ```

#### 正規化ルール

辞書登録時とトークンマッチング時の両方で同じ正規化を適用する:

1. 小文字化（ASCII のみ）
2. 全角英数字 → 半角変換
3. 記号類の除去: `:`, `：`, `・`, `~`, `～`, `!`, `！`, `?`, `？`
4. 連続スペースの正規化

```
例:
  "流星ワールドアクター Gaslight Bullet" → "流星ワールドアクター gaslight bullet"
  "Ryuusei World Actor: Gaslight Bullet" → "ryuusei world actor gaslight bullet"
  "12の月のイヴ"                         → "12の月のイヴ"
  "FuriKuru"                             → "furikuru"
```

### マッチングアルゴリズム

Step 2 で生成した各トークンを正規化し、辞書に対して以下の **3 種類のマッチング**を試行する。

#### 3-1. 完全一致

```
normalized(token) == normalized(辞書の名前)
```

スコア: **1.0**

#### 3-2. 包含一致（トークンが名前に含まれる）

```
normalized(token) in normalized(辞書の名前)
```

例: トークン `流星ワールドアクター` がタイトル `流星ワールドアクター gaslight bullet` に含まれる

スコア: `len(token) / len(辞書の名前)` （カバー率）

#### 3-3. 包含一致（名前がトークンに含まれる）

```
normalized(辞書の名前) in normalized(token)
```

例: トークン `流星ワールドアクターGB` にタイトル `流星ワールドアクター` が含まれる（Latin 除去後に一致するケース等）

スコア: `len(辞書の名前) / len(token)` （カバー率）

#### 3-3. 包含一致（名前がトークンに含まれる）

```
normalized(辞書の名前) in normalized(token)
```

例: トークン `流星ワールドアクターGB` にタイトル `流星ワールドアクター` が含まれる（Latin 除去後に一致するケース等）

スコア: `len(辞書の名前) / len(token)` （カバー率）

### マッチ結果の構造

```python
@dataclass
class MatchResult:
    game_id: str          # "v60196"
    match_type: str       # "title" | "title_latin" | "alias" | "brand" | "brand_latin" | "brand_alias"
    match_method: str     # "exact" | "token_in_name" | "name_in_token"
    score: float          # 0.0 〜 1.0
    token: str            # マッチしたトークン
    matched_name: str     # マッチした辞書の名前
```

---

## Step 4: スコアリング・選定

### 4-1. 候補の集約

同一 `game_id` の `MatchResult` をグルーピングし、以下を算出:

```python
for game_id, matches in grouped_results:
    # match_type の優先度に基づく重み付け
    type_weight = {
        "title": 1.0,         # タイトル完全一致が最も信頼できる
        "title_latin": 0.95,
        "alias": 0.9,
        "brand": 0.6,         # ブランド名だけでは特定できない（複数タイトルがある）
        "brand_latin": 0.55,
        "brand_alias": 0.5,
    }

    # match_method の基礎点（完全一致を明確に優遇）
    method_base = {
        "exact": 1.0,
        "token_in_name": 0.7,
        "name_in_token": 0.7,
    }

    for m in matches:
        m.weighted_score = m.score * type_weight[m.match_type] * method_base[m.match_method]

    best_score = max(m.weighted_score for m in matches)
    
    # ブランドとタイトルの両方がヒットしたらボーナス
    has_title_match = any(m.match_type.startswith("title") or m.match_type == "alias" for m in matches)
    has_brand_match = any(m.match_type.startswith("brand") for m in matches)
    cross_bonus = 0.1 if (has_title_match and has_brand_match) else 0.0
    
    final_score = best_score + cross_bonus
```

### 4-2. 過剰包含のペナルティ (Overlapping Title Penalty)

文字列の包含マッチを利用しているため、「無印版」と「拡張版（続編など）」が辞書内に両方存在する場合、拡張版のファイル・パスから無印版にも同時に包含マッチしてしまいます。
これを防ぐため、**「A が B を包含している（A ⊃ B）」関係にある辞書エントリが複数マッチした場合、短い方（包含されている側）にペナルティを与え**、長い方（より具体的な方）のスコアが競り勝つように調整します。

例: `流星ワールドアクターGB` というトークンが以下の両方に `name_in_token` でマッチした場合
* A: `流星ワールドアクター Gaslight Bullet` (の略称展開後)
* B: `流星ワールドアクター`

このとき、A ⊃ B の関係にあるため、B (無印版) にのみペナルティを掛けることで、GB版が正しく最上位に選定されるようにします。

```python
# 1. 共通の文字数差分による減衰 (ベース)
len_diff = abs(len(normalized_token) - len(normalized_name))
proximity_factor = 1.0 / (1.0 + len_diff * 0.05)
m.weighted_score *= proximity_factor

# 2. 重複を含む場合の長いタイトル優先ペナルティ
# （同一ゲームID群を越えて、全体のマッチ結果を俯瞰して処理する）
for m_short in all_matches:
    for m_long in all_matches:
        if m_short.game_id != m_long.game_id:
            if m_short.matched_name in m_long.matched_name: # LCPのような包含関係があるか
                # 短い方のスコアを大幅に下げる
                m_short.weighted_score *= 0.3
```

### 4-3. 閾値フィルタ

- `final_score < 0.4` の候補は棄却
- 閾値は要チューニング

### 4-3. window_title による補強（オプション）

`window_title` が利用可能な場合、同じ辞書マッチングを `window_title` にも適用する。
`process_path` と `window_title` の結果が同一 `game_id` を指していればさらに確信度を上げる。

### 4-4. 出力

```json
{
  "identified": true,
  "game_id": "v60196",
  "official_title": "流星ワールドアクター Gaslight Bullet",
  "brand": "Heliodor",
  "confidence": 0.92,
  "match_details": [
    { "token": "流星ワールドアクター", "matched": "流星ワールドアクター Gaslight Bullet", "type": "title", "method": "token_in_name", "score": 0.68 },
    { "token": "Heliodor", "matched": "Heliodor", "type": "brand", "method": "exact", "score": 0.60 }
  ]
}
```

---

## 各ケースでの想定動作

### Case 1: `G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe`

Step 2 で生成されたトークンの中から以下がヒットする:

| トークン | マッチ方法 | マッチ先 | match_type | スコア |
|---|---|---|---|---|
| `Heliodor` | 完全一致 | Brand `Heliodor` | brand | 0.60 |
| `流星ワールドアクターgb` | 包含(token_in_name) | Title `流星ワールドアクター Gaslight Bullet` (略称展開後と仮定) | title | 0.90 |
| `流星ワールドアクターgb` | 包含(name_in_token) | Title (無印版) `流星ワールドアクター` | title | 0.17 |

→ 無印版への包含マッチは、「過剰包含のペナルティ」により `流星ワールドアクター Gaslight Bullet` 側の存在が検知され大幅減衰(0.17付近まで低下)されるため誤爆しない。
→ title + brand 両方ヒット → cross_bonus +0.1 → **final_score ≈ 1.0 → 正解 (`流星ワールドアクター Gaslight Bullet` に同定)**

### Case 2: `G:\game\いつか、届く、あの空に。\main.bin`

| トークン | マッチ方法 | マッチ先 | match_type | スコア |
|---|---|---|---|---|
| `いつか、届く、あの空に。` | 完全一致 | Title `いつか、届く、あの空に。` | title | **1.0** |
| `main` | (マッチなし) | — | — | — |

→ title 完全一致 → **final_score = 1.0 → 正解**

### Case 3: `G:\game\FuriKuru_Game\諦観のイヴ・ベセル\FuriKuru01.exe`

| トークン | マッチ方法 | マッチ先 | match_type | スコア |
|---|---|---|---|---|
| `FuriKuru` | 完全一致 | Brand Latin `FuriKuru` | brand_latin | 0.55 |
| `Furi Kuru` | 包含(token_in_name) | Brand Latin `FuriKuru` (正規化後一致) | brand_latin | 0.55 |
| `諦観のイヴ・ベセル` | 完全一致 | Title `諦観のイヴ・ベセル` | title | **1.0** |

→ title + brand 両方ヒット → cross_bonus +0.1 → **final_score ≈ 1.1 → 正解**

### Case 4: `G:\game\trinoline\trinoline.exe`

| トークン | マッチ方法 | マッチ先 | match_type | スコア |
|---|---|---|---|---|
| `trinoline` | 完全一致 | Title `Trinoline` (正規化後一致) | title | **1.0** |
| `trinoline` | 完全一致 | Title Latin `Trinoline` (正規化後一致) | title_latin | 0.95 |

→ **final_score = 1.0 → 正解**

### Case 5: `F:\game\12eve\12eve.exe`

| トークン | マッチ方法 | マッチ先 | match_type | スコア |
|---|---|---|---|---|
| `12eve` | 包含(token_in_name) | Alias `12mooneve` | alias | 0.45 |
| `12eve` | 包含(name_in_token) | Alias (なし、短い) | — | — |
| `12 eve` | 包含(token_in_name) | Title `Eve of the 12 Months` | title | 0.27 |
| `12 eve` | 包含(token_in_name) | Title Latin `12 no Tsuki no Eve` | title_latin | 0.30 |

→ スコアが低い。**このケースは文字列マッチングだけでは困難**。

**対策案:**
- `window_title` からの補強が有効（ウィンドウタイトルには日本語タイトル `12の月のイヴ` が表示されている可能性が高い）
- Alias の部分一致 `12eve` ⊂ `12mooneve` で最低限のヒントは得られる

### Case 6: `F:\game\サクラノ刻\sakuranotoki.exe`

Step 2 の生成トークンの中に `サクラノ刻`, `sakuranotoki` が含まれる。（辞書側にも略称・サフィックス除去ルールが適用される）

| トークン | マッチ方法 | マッチ先 | match_type | スコア |
|---|---|---|---|---|
| `サクラノ刻` | 完全一致 | Title `サクラノ刻－櫻の森の下を歩む－` (サブタイトル結合前の派生名) | title | 1.00 |
| `sakuranotoki` | 完全一致 | Title_Latin `Sakura no Toki` (スペース除去パターンの派生名) | title_latin | 0.95 |

→ **final_score = 1.0 → 正解 (`サクラノ刻－櫻の森の下を歩む－` に同定)**

---

## 実装の段階

### Phase 1: Python スクリプトで PoC

`scripts/` 配下に Python スクリプトを作成し、パイプラインの妥当性を検証する。

- 入力: コマンドライン引数でパスを受け取る
- 処理: Step 1〜4 を順に実行（VNDB の TSV データを直接読み込み）
- 出力: 推定結果を表示
- 既存の `upsert_qdrant.py` の `load_data` / `extract_metadata` を再利用

```bash
$env:PYTHONIOENCODING="utf-8"; uv run python identify_title.py "G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe"
```

### Phase 2: Rust 側に組み込み

PoC で精度が確認できたら、hostd の Rust 側に同等のロジックを実装する。

- VNDB データを起動時に読み込み、正規化辞書をメモリ上に構築
- `ScreenshotMetadataPayload` に `game_id` / `official_title` フィールドを追加
- プロセスパス変更時に 1 回だけ推定を実行しキャッシュ

### Phase 3: Android 側でのUI表示

推定されたゲーム情報をギャラリー画面でグルーピング表示に活用する。

---

## 注意事項・課題

1. **辞書サイズ**: VNDB 全データでは数万件のゲーム × 複数名前 = 数十万エントリ。正規化済み辞書を HashMap に格納すれば完全一致は O(1)。包含一致はフルスキャンになるため、短いトークンへの対策が必要
2. **同名ゲームの区別**: 同一ブランドの続編など、似た名前のゲームが複数ヒットする場合がある。cross_bonus（ブランド+タイトル）で緩和
3. **`12eve` のような短い略称**: 文字列マッチングではスコアが低くなる。`window_title` からの補強が重要
4. **正規化の過剰/不足**: 記号を除去しすぎると誤マッチが増え、除去しなさすぎるとマッチしない。チューニングが必要
