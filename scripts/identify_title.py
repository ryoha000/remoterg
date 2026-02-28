import os
import sys
import argparse
import re
import pandas as pd
import unicodedata
from dataclasses import dataclass
from typing import List, Dict, Set, Tuple, Optional

# --- Constants from flow document ---

GENERIC_DIR_NAMES = {
    "game", "games", "program files", "program files (x86)", "eroge", 
    "visual novel", "vn", "users", "desktop", "download", "bin", "lib", "data"
}

# Derived from VNDB engines potentially, but specified in doc
ENGINE_NAMES = {
    "siglusengine", "bgi", "ethornell", "kirikiri", "rio", "alpharomdis", 
    "catsystem2", "yu-ris", "majiro", "advhd", "qlie"
}

UTILITY_FILENAMES = {
    "setup", "install", "uninst", "uninstall", "patch", "update", 
    "launcher", "config", "readme", "main"
}

GENERIC_SUBWORDS = {"game", "app", "launcher"}

# --- Data Structures ---

@dataclass
class MatchResult:
    game_id: str
    match_type: str       # "title" | "title_latin" | "alias" | "brand" | "brand_latin" | "brand_alias"
                          # 略称バリエーション由来の場合は ":generated" サフィックス付き
                          # 例: "title:generated", "title_latin:generated"
    match_method: str     # "exact" | "token_in_name" | "name_in_token"
    score: float          # Coverage score
    token: str            # Token that matched
    matched_name: str     # Name in dictionary that matched
    derived_rule: str     # "Rule A", "Rule B", etc.
    weighted_score: float = 0.0

# --- Step 1: Path Segment Extraction ---

def extract_segments(process_path: str) -> List[str]:
    # Replace backslashes with slashes for uniform splitting
    path = process_path.replace("\\", "/")
    parts = [p for p in path.split("/") if p]
    
    segments = []
    for i, part in enumerate(parts):
        # Remove drive letter (e.g., "C:")
        if i == 0 and re.match(r"^[a-zA-Z]:$", part):
            continue
            
        part_lower = part.lower()
        
        # Skip generic directory names
        if part_lower in GENERIC_DIR_NAMES:
            continue
            
        # For the last element (filename), check engine names and utility names
        if i == len(parts) - 1:
            name_no_ext = os.path.splitext(part_lower)[0]
            if name_no_ext in ENGINE_NAMES or name_no_ext in UTILITY_FILENAMES:
                continue
        
        segments.append(part)
        
    return segments

# --- Step 2: Preprocessing and Normalization (Token Generation) ---

def generate_tokens(segments: List[str]) -> List[Tuple[str, str]]:
    all_tokens = set() # Store tuples: (token_str, derived_rule)
    
    for segment in segments:
        tokens_for_segment = {(segment, "Original")}
        
        # Rule A: Remove extensions
        current_segment = segment
        name, ext = os.path.splitext(segment)
        if ext.lower() in {".exe", ".bin", ".log", ".bat", ".lnk"}:
            current_segment = name
            tokens_for_segment.add((current_segment, "Rule A"))
            if (segment, "Original") in tokens_for_segment and segment != current_segment:
                tokens_for_segment.remove((segment, "Original"))
        
        new_tokens = set()
        for t, rule in tokens_for_segment:
            # Rule B: Split by separators (_ , -, etc)
            if re.search(r'[_\-－―～~]', t):
                spaced = re.sub(r'[_\-－―～~]+', ' ', t).strip()
                if spaced:
                    new_tokens.add((spaced, "Rule B"))
                
                match = re.search(r'([_\-－―～~]+)', t)
                if match:
                    prefix = t[:match.start()].strip()
                    if prefix:
                        new_tokens.add((prefix, "Rule B"))
            
            # Rule C: CamelCase split
            camel_split = re.sub(r'([a-z])([A-Z])', r'\1 \2', t)
            camel_split = re.sub(r'([A-Z])([A-Z][a-z])', r'\1 \2', camel_split)
            if camel_split != t:
                new_tokens.add((camel_split, "Rule C"))
                
            # Rule D: Character type boundary split (JP <-> ASCII, Alpha <-> Digit)
            type_split = re.sub(r'([^\x00-\x7F])([a-zA-Z0-9])', r'\1 \2', t)
            type_split = re.sub(r'([a-zA-Z0-9])([^\x00-\x7F])', r'\1 \2', type_split)
            type_split = re.sub(r'([a-zA-Z])([0-9])', r'\1 \2', type_split)
            type_split = re.sub(r'([0-9])([a-zA-Z])', r'\1 \2', type_split)
            if type_split != t:
                new_tokens.add((type_split, "Rule D"))
                
        tokens_for_segment.update(new_tokens)
        
        # ルール E（旧 G）: ジェネリックサブワード除去
        final_new_tokens = set()
        for t, rule in tokens_for_segment:
            words = t.split()
            if len(words) > 1:
                filtered_words = [w for w in words if w.lower() not in GENERIC_SUBWORDS]
                if filtered_words and len(filtered_words) < len(words):
                    final_new_tokens.add((" ".join(filtered_words), "Rule E"))
        
        tokens_for_segment.update(final_new_tokens)
        all_tokens.update(tokens_for_segment)
        
    return sorted(list(all_tokens))

# --- Normalization for Matching ---

def normalize_for_match(text: str) -> str:
    if not text:
        return ""
    # 1. Lowercase ASCII
    text = text.lower()
    # 2. Fullwidth to Halfwidth
    text = unicodedata.normalize('NFKC', text)
    # 3. Remove symbols (keep only word characters and whitespaces)
    text = re.sub(r'[^\w\s]', ' ', text)
    text = text.replace('_', ' ')
    # 4. Space normalization
    text = " ".join(text.split())
    return text

# --- Step 3: Dictionary Construction and Matching ---

class TitleDictionary:
    def __init__(self):
        self.entries: Dict[str, List[Tuple[str, str, str]]] = {} # normalized -> [(id, type, original)]

    def add(self, name: str, game_id: str, match_type: str):
        """辞書にエントリを追加する。"""
        if not name or name == '\\N':
            return
        norm = normalize_for_match(name)
        if not norm:
            return
        if norm not in self.entries:
            self.entries[norm] = []
        self.entries[norm].append((game_id, match_type, name))

    def add_generated_variants(self, name: str, game_id: str, match_type: str):
        """名前から略称バリエーションを自動生成して :generated 付きで辞書に追加する。"""
        if not name or name == '\\N':
            return
        norm = normalize_for_match(name)
        if not norm:
            return
        
        generated_type = f"{match_type}:generated"
        words = norm.split()
        
        if len(words) > 1:
            # 頭文字結合: 各単語の頭文字を小文字で結合
            # 数字のみの単語はそのまま保持（例: "2" → "2"）
            all_initials = "".join(
                w if re.match(r'^[0-9]+$', w) else w[0]
                for w in words
            )
            if len(all_initials) >= 2:
                self.add(all_initials, game_id, generated_type)
            
            # 先頭語＋以降の頭文字: 最初の単語を残し、2番目以降の頭文字を結合
            rest_initials = "".join(
                w if re.match(r'^[0-9]+$', w) else w[0]
                for w in words[1:]
            )
            first_plus_initials = words[0] + rest_initials
            if first_plus_initials != norm:
                self.add(first_plus_initials, game_id, generated_type)
            
            # スペース除去（密着）: 全スペースを除去して連結
            no_space = norm.replace(" ", "")
            if no_space != norm:
                self.add(no_space, game_id, generated_type)
        
        # サブタイトル除去: 元の名前で「－」「―」「-」の前までを抽出
        # （正規化前の元テキストから処理し、結果を正規化して登録）
        subtitle_match = re.search(r'[－―\-]', name)
        if subtitle_match:
            prefix = name[:subtitle_match.start()].strip()
            if prefix and prefix != name:
                self.add(prefix, game_id, generated_type)

    def match(self, token: str) -> List[MatchResult]:
        """トークンを辞書とマッチングする。ハッシュマップ完全一致 + 包含一致のみ。"""
        norm_token = normalize_for_match(token)
        if not norm_token:
            return []
            
        results = []
        
        # 辞書全体をイテレートして包含マッチも試行する
        # 注: 本番（Rust）ではより最適化すること
        for norm_name, list_info in self.entries.items():
            method = None
            score = 0.0
            
            # 1. 完全一致
            if norm_token == norm_name:
                method = "exact"
                score = 1.0
            else:
                # スペース無視の文字列を準備
                token_no_space = norm_token.replace(" ", "")
                name_no_space = norm_name.replace(" ", "")
                
                # 2. スペース無視完全一致
                if token_no_space == name_no_space and token_no_space:
                    method = "exact_space_agnostic"
                    score = 0.95
                
                # 3. 前方一致
                elif norm_name.startswith(norm_token) and len(norm_token) >= 3:
                    method = "prefix"
                    score = 0.85 * (len(norm_token) / len(norm_name)) ** 0.1
                
                # 4. 包含一致（トークンが名前に含まれる）
                elif norm_token in norm_name:
                    method = "token_in_name"
                    score = len(norm_token) / len(norm_name)
                    
                # 5. 包含一致（名前がトークンに含まれる）
                elif norm_name in norm_token:
                    method = "name_in_token"
                    score = len(norm_name) / len(norm_token)
                    
                # 6. スペース無視包含一致
                elif token_no_space in name_no_space and len(token_no_space) >= 5:
                    method = "token_in_name_space_agnostic"
                    score = 0.95 * (len(token_no_space) / len(name_no_space))
                elif name_no_space in token_no_space and len(name_no_space) >= 5:
                    method = "name_in_token_space_agnostic"
                    score = 0.95 * (len(name_no_space) / len(token_no_space))
            
            if method:
                for game_id, match_type, original_name in list_info:
                    results.append(MatchResult(
                        game_id=game_id,
                        match_type=match_type,
                        match_method=method,
                        score=score,
                        token=token,
                        matched_name=original_name,
                        derived_rule="Unknown" # 呼び出し元で設定
                    ))
                    
        return results

def load_vndb_dictionary(data_dir: str) -> Tuple[TitleDictionary, Dict[str, str]]:
    print(f"VNDB データを読み込んでいます: {data_dir}")
    
    # Load titles
    vn_titles = pd.read_csv(os.path.join(data_dir, 'vn_titles'), sep='\t', names=[
        'id', 'lang', 'official', 'title', 'latin'
    ], keep_default_na=False, dtype=str)
    
    # Determine best official title for each ID (JA Official > JA Any > Official Any > Any)
    # This logic mimics upsert_qdrant.py for consistency
    title_ja_official = vn_titles[(vn_titles['lang'] == 'ja') & (vn_titles['official'] == 't')].groupby('id')['title'].first()
    title_ja = vn_titles[vn_titles['lang'] == 'ja'].groupby('id')['title'].first()
    title_official = vn_titles[vn_titles['official'] == 't'].groupby('id')['title'].first()
    title_any = vn_titles.groupby('id')['title'].first()
    
    official_title_map = title_ja_official.combine_first(
        title_ja.combine_first(
            title_official.combine_first(title_any)
        )
    ).to_dict()
    
    # Load aliases from VN
    vn = pd.read_csv(os.path.join(data_dir, 'vn'), sep='\t', usecols=[0, 11], names=[
        'id', 'alias'
    ], keep_default_na=False, dtype=str)
    
    # Load producer info for brands
    producers = pd.read_csv(os.path.join(data_dir, 'producers'), sep='\t', names=[
        'id', 'type', 'lang', 'name', 'latin', 'alias', 'description'
    ], keep_default_na=False, dtype=str)
    
    # Load release and developer linkage
    releases_vn = pd.read_csv(os.path.join(data_dir, 'releases_vn'), sep='\t', names=[
        'id', 'vid', 'rtype'
    ], keep_default_na=False, dtype=str)
    
    releases_producers = pd.read_csv(os.path.join(data_dir, 'releases_producers'), sep='\t', names=[
        'id', 'pid', 'developer', 'publisher'
    ], keep_default_na=False, dtype=str)

    # Link VNs to Producers (Developers)
    rel_prod = pd.merge(releases_vn, releases_producers, on='id')
    developers = rel_prod[rel_prod['developer'] == 't']
    
    # Build dictionary
    dic = TitleDictionary()
    
    print("タイトルを辞書に登録しています...")
    for _, row in vn_titles.iterrows():
        dic.add(row['title'], row['id'], 'title')
        dic.add(row['latin'], row['id'], 'title_latin')
        # 略称バリエーションを自動生成して辞書に追加
        dic.add_generated_variants(row['title'], row['id'], 'title')
        dic.add_generated_variants(row['latin'], row['id'], 'title_latin')
        
    print("エイリアスを辞書に登録しています...")
    for _, row in vn.iterrows():
        aliases = row['alias'].replace('\\N', '').split('\\n')
        for a in aliases:
            if a.strip():
                dic.add(a.strip(), row['id'], 'alias')
                dic.add_generated_variants(a.strip(), row['id'], 'alias')
                
    print("ブランド名を辞書に登録しています...")
    # pid を VN id にマッピング
    dev_map = developers.groupby('pid')['vid'].apply(list).to_dict()
    
    for _, row in producers.iterrows():
        pid = row['id']
        if pid not in dev_map:
            continue
            
        vids = dev_map[pid]
        brand_names = []
        if row['name'] and row['name'] != '\\N': brand_names.append((row['name'], 'brand'))
        if row['latin'] and row['latin'] != '\\N': brand_names.append((row['latin'], 'brand_latin'))
        
        aliases = row['alias'].replace('\\N', '').split('\\n')
        for a in aliases:
            if a.strip():
                brand_names.append((a.strip(), 'brand_alias'))
                
        for vid in vids:
            for name, mtype in brand_names:
                dic.add(name, vid, mtype)
                # ブランド名の略称バリエーションも自動生成
                dic.add_generated_variants(name, vid, mtype)
                
    entry_count = sum(len(v) for v in dic.entries.values())
    print(f"辞書構築完了: {len(dic.entries)} 正規化キー, {entry_count} エントリ")
    return dic, official_title_map

# --- Step 4: Scoring and Selection ---

def score_and_select(matches: List[MatchResult], official_title_map: Dict[str, str]) -> Dict:
    if not matches:
        return {"identified": False}
        
    type_weight = {
        "title": 1.0,
        "title_latin": 0.95,
        "alias": 0.9,
        "brand": 0.6,
        "brand_latin": 0.55,
        "brand_alias": 0.5,
    }
    
    method_base = {
        "exact": 1.0,
        "exact_space_agnostic": 0.95,
        "prefix": 0.85,
        "token_in_name": 0.7,
        "name_in_token": 0.7,
        "token_in_name_space_agnostic": 0.65,
        "name_in_token_space_agnostic": 0.65,
    }
    
    # game_id でグルーピング
    grouped: Dict[str, List[MatchResult]] = {}
    for m in matches:
        if m.game_id not in grouped:
            grouped[m.game_id] = []
        grouped[m.game_id].append(m)
        
    candidates = []
    for game_id, game_matches in grouped.items():
        for m in game_matches:
            # ":generated" サフィックスを除去して type_weight を参照（重みは原典と同一）
            base_type = m.match_type.split(":")[0]
            w_score = m.score * type_weight[base_type] * method_base[m.match_method]
            
            # 4-2: 包含マッチの近接ペナルティ
            if "exact" not in m.match_method and m.match_method != "prefix":
                norm_token = normalize_for_match(m.token)
                norm_name = normalize_for_match(m.matched_name)
                len_diff = abs(len(norm_token) - len(norm_name))
                proximity_factor = 1.0 / (1.0 + len_diff * 0.05)
                w_score *= proximity_factor
            
            m.weighted_score = w_score

    # Apply Rule 4-2: Overlapping Title Penalty (Cross-Group)
    all_structured_matches = [m for group in grouped.values() for m in group]
    
    # 候補同士でのペナルティ判定
    for m_short in all_structured_matches:
        # 包含マッチ(name_in_token)の場合は純粋なペナルティ
        if m_short.match_method == "name_in_token":
            norm_short_name = normalize_for_match(m_short.matched_name)
            for m_long in all_structured_matches:
                if m_short.game_id != m_long.game_id:
                    norm_long_name = normalize_for_match(m_long.matched_name)
                    if norm_short_name in norm_long_name and len(norm_long_name) > len(norm_short_name):
                        m_short.weighted_score *= 0.3
                        
    for game_id, game_matches in grouped.items():
        best_match = max(game_matches, key=lambda x: x.weighted_score)
        
        # Rule 4-1: Brand + Title bonus
        # ":generated" を除去したベース型で判定
        has_title_match = any(m.match_type.split(":")[0].startswith("title") or m.match_type.split(":")[0] == "alias" for m in game_matches)
        has_brand_match = any(m.match_type.split(":")[0].startswith("brand") for m in game_matches)
        cross_bonus = 0.1 if (has_title_match and has_brand_match) else 0.0
        
        final_score = best_match.weighted_score + cross_bonus
        
        candidates.append({
            "game_id": game_id,
            "final_score": final_score,
            "best_match": best_match,
            "all_matches": game_matches
        })
        
    candidates.sort(key=lambda x: x['final_score'], reverse=True)
    
    # Filter by threshold 0.4
    valid_candidates = [c for c in candidates if c['final_score'] >= 0.4]
    
    if not valid_candidates:
        return {"identified": False, "top_candidate": candidates[0] if candidates else None}
        
    top = valid_candidates[0]
    return {
        "identified": True,
        "game_id": top["game_id"],
        "official_title": official_title_map.get(top["game_id"], "Unknown"),
        "confidence": top["final_score"],
        "match_details": [
            {
                "token": m.token,
                "matched": m.matched_name,
                "type": m.match_type,
                "method": m.match_method,
                "score": round(m.weighted_score, 3),
                "rule": m.derived_rule
            }
            for m in top["all_matches"]
        ]
    }

# --- Main Flow ---

def main():
    parser = argparse.ArgumentParser(description="Process path からゲームタイトルを推定する PoC")
    parser.add_argument("process_path", help="ゲームのプロセスパス")
    parser.add_argument("--data-dir", default=r"data\vndb-db-2026-02-28\extracted\db", help="VNDB データのディレクトリ")
    args = parser.parse_args()
    
    print(f"Input Process Path: {args.process_path}")
        
    # Step 1: Segments
    segments = extract_segments(args.process_path)
    print(f"Step 1 Segments: {segments}")
    
    # Step 2: Tokens
    tokens = generate_tokens(segments)
    print(f"Step 2 Tokens: {tokens}")
    
    # Step 3: Load Data and Match
    dic, official_title_map = load_vndb_dictionary(args.data_dir)
    
    all_matches = []
    print("辞書マッチングを実行中...")
    for token_str, derived_rule in tokens:
        matches = dic.match(token_str)
        # Inherit the generated rule flag from token generation
        for m in matches:
            m.derived_rule = derived_rule
        all_matches.extend(matches)
        
    # Step 4: Scoring
    result = score_and_select(all_matches, official_title_map)
    
    if result["identified"]:
        print("\n=== 推定結果 ===")
        print(f"Game ID: {result['game_id']}")
        print(f"Title: {result['official_title']}")
        print(f"Confidence: {result['confidence']:.3f}")
        print("Details:")
        for det in result["match_details"]:
            print(f"  - [{det['type']}] '{det['token']}' ({det['rule']}) -> '{det['matched']}' ({det['method']}, score: {det['score']})")
    else:
        print("\nタイトルを特定できませんでした。")
        if "top_candidate" in result and result["top_candidate"]:
            top = result["top_candidate"]
            print(f"最高スコア候補: {top['game_id']} (Score: {top['final_score']:.3f})")

if __name__ == "__main__":
    main()
