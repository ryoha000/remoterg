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
    match_method: str     # "exact" | "token_in_name" | "name_in_token"
    score: float          # Coverage score
    token: str            # Token that matched
    matched_name: str     # Name in dictionary that matched
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

def generate_tokens(segments: List[str]) -> List[str]:
    all_tokens = set()
    
    for segment in segments:
        tokens_for_segment = {segment}
        
        # Rule A: Remove extensions
        current_segment = segment
        name, ext = os.path.splitext(segment)
        if ext.lower() in {".exe", ".bin", ".log", ".bat", ".lnk"}:
            current_segment = name
            tokens_for_segment.add(current_segment)
            if segment in tokens_for_segment and segment != current_segment:
                tokens_for_segment.remove(segment)
        
        new_tokens = set()
        for t in tokens_for_segment:
            # Rule B: Split by separators (_ , -)
            # Add space-separated version and also "before-separator" as subtitle separation
            if "_" in t or "-" in t:
                # Replace with space
                spaced = t.replace("_", " ").replace("-", " ")
                new_tokens.add(spaced)
                
                # Subtitle split: take before the first dash/underscore
                match = re.search(r'([_-]+)', t)
                if match:
                    prefix = t[:match.start()].strip()
                    if prefix:
                        new_tokens.add(prefix)
            
            # Rule C: CamelCase split
            camel_split = re.sub(r'([a-z])([A-Z])', r'\1 \2', t)
            camel_split = re.sub(r'([A-Z])([A-Z][a-z])', r'\1 \2', camel_split)
            if camel_split != t:
                new_tokens.add(camel_split)
                
            # Rule D: Character type boundary split (JP <-> ASCII, Alpha <-> Digit)
            type_split = re.sub(r'([^\x00-\x7F])([a-zA-Z0-9])', r'\1 \2', t)
            type_split = re.sub(r'([a-zA-Z0-9])([^\x00-\x7F])', r'\1 \2', type_split)
            type_split = re.sub(r'([a-zA-Z])([0-9])', r'\1 \2', type_split)
            type_split = re.sub(r'([0-9])([a-zA-Z])', r'\1 \2', type_split)
            if type_split != t:
                new_tokens.add(type_split)
                
        tokens_for_segment.update(new_tokens)
        
        # Rule E: Acronym / Initialisms & Space Removal
        # Operates on the generated token sets (e.g. spaced out tokens)
        acronym_tokens = set()
        for t in tokens_for_segment:
            # All spaces removed (Space-Agnostic base)
            no_space = t.replace(" ", "")
            if no_space != t:
                acronym_tokens.add(no_space)
                
            words = t.split()
            if len(words) > 1:
                # All initials: WHITE ALBUM 2 -> wa2
                all_initials = "".join([w[0] if not re.match(r'[0-9]', w) else w for w in words])
                acronym_tokens.add(all_initials)
                
                # First word + initials of rest: 流星ワールドアクター Gaslight Bullet -> 流星ワールドアクターgb
                first_plus_initials = words[0] + "".join([w[0] if not re.match(r'[0-9]', w) else w for w in words[1:]])
                acronym_tokens.add(first_plus_initials)

        tokens_for_segment.update(acronym_tokens)
        
        # Rule F: Generic subword removal (from Rule B results etc.)
        final_new_tokens = set()
        for t in tokens_for_segment:
            # We don't remove if it results in empty token
            words = t.split()
            if len(words) > 1:
                filtered_words = [w for w in words if w.lower() not in GENERIC_SUBWORDS]
                if filtered_words and len(filtered_words) < len(words):
                    final_new_tokens.add(" ".join(filtered_words))
        
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
    # 3. Remove symbols: : ： ・ ~ ～ ! ！ ? ？
    text = re.sub(r'[:：・~～!！?？]', ' ', text)
    # 4. Space normalization
    text = " ".join(text.split())
    return text

# --- Step 3: Dictionary Construction and Matching ---

class TitleDictionary:
    def __init__(self):
        self.entries: Dict[str, List[Tuple[str, str, str]]] = {} # normalized -> [(id, type, original)]

    def add(self, name: str, game_id: str, match_type: str):
        if not name or name == '\\N':
            return
        norm = normalize_for_match(name)
        if not norm:
            return
        if norm not in self.entries:
            self.entries[norm] = []
        self.entries[norm].append((game_id, match_type, name))

    def match(self, token: str) -> List[MatchResult]:
        norm_token = normalize_for_match(token)
        if not norm_token:
            return []
            
        results = []
        
        # We need to iterate over the dictionary for inclusion matching
        # Note: In production (Rust), this should be optimized.
        for norm_name, list_info in self.entries.items():
            method = None
            score = 0.0
            
            # 1. Exact Match
            if norm_token == norm_name:
                method = "exact"
                score = 1.0
            else:
                # Prepare Space-Agnostic strings
                token_no_space = norm_token.replace(" ", "")
                name_no_space = norm_name.replace(" ", "")
                
                # 2. Space-Agnostic Exact Match
                if token_no_space == name_no_space and token_no_space:
                    method = "exact_space_agnostic"
                    score = 0.95
                
                # 3. Prefix Match
                elif norm_name.startswith(norm_token) and len(norm_token) >= 3:
                    method = "prefix"
                    # High base score (0.85) to avoid length coverage penalty of regular inclusion
                    score = 0.85 * (len(norm_token) / len(norm_name)) ** 0.1 # Slight length sway
                
                # 4. Inclusion Match (Token in Name)
                elif norm_token in norm_name:
                    method = "token_in_name"
                    score = len(norm_token) / len(norm_name)
                    
                # 5. Inclusion Match (Name in Token)
                elif norm_name in norm_token:
                    method = "name_in_token"
                    score = len(norm_name) / len(norm_token)
                    
                # 6. Space-Agnostic Inclusion
                elif token_no_space in name_no_space and len(token_no_space) >= 5:
                    method = "token_in_name_space_agnostic"
                    score = 0.95 * (len(token_no_space) / len(name_no_space))
                elif name_no_space in token_no_space and len(name_no_space) >= 5:
                    method = "name_in_token_space_agnostic"
                    score = 0.95 * (len(name_no_space) / len(token_no_space))
                    
                # 7. Initialism Match (LCP + Initials of remaining words)
                if not method and len(norm_token) > 3 and len(norm_name) > 3:
                    # Find Longest Common Prefix (at least 3 chars)
                    lcp_len = 0
                    for c1, c2 in zip(norm_token, norm_name):
                        if c1 == c2: lcp_len += 1
                        else: break
                        
                    if lcp_len >= 3:
                        token_remainder = norm_token[lcp_len:].strip()
                        name_remainder = norm_name[lcp_len:].strip()
                        if token_remainder and name_remainder:
                            # Check if token remainder is acronym of name remainder
                            name_words = name_remainder.split()
                            if len(name_words) > 1:
                                initials = "".join([w[0] for w in name_words if w])
                                if token_remainder == initials:
                                    method = "initialism"
                                    score = 0.90
            
            if method:
                for game_id, match_type, original_name in list_info:
                    results.append(MatchResult(
                        game_id=game_id,
                        match_type=match_type,
                        match_method=method,
                        score=score,
                        token=token,
                        matched_name=original_name
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
        
    print("エイリアスを辞書に登録しています...")
    for _, row in vn.iterrows():
        aliases = row['alias'].replace('\\N', '').split('\\n')
        for a in aliases:
            if a.strip():
                dic.add(a.strip(), row['id'], 'alias')
                
    print("ブランド名を辞書に登録しています...")
    # Map pid to VN ids
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
        "initialism": 0.9,
        "prefix": 0.85,
        "token_in_name": 0.7,
        "name_in_token": 0.7,
        "token_in_name_space_agnostic": 0.65,
        "name_in_token_space_agnostic": 0.65,
    }
    
    # Group by game_id
    grouped: Dict[str, List[MatchResult]] = {}
    for m in matches:
        if m.game_id not in grouped:
            grouped[m.game_id] = []
        grouped[m.game_id].append(m)
        
    candidates = []
    for game_id, game_matches in grouped.items():
        for m in game_matches:
            # Basic weighted score
            w_score = m.score * type_weight[m.match_type] * method_base[m.match_method]
            
            # Rule 4-2: Proximity penalty for inclusion matches (base penalty)
            if "exact" not in m.match_method and m.match_method not in ("initialism", "prefix"):
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
        has_title_match = any(m.match_type.startswith("title") or m.match_type == "alias" for m in game_matches)
        has_brand_match = any(m.match_type.startswith("brand") for m in game_matches)
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
                "score": round(m.weighted_score, 3)
            }
            for m in top["all_matches"]
        ]
    }

# --- Main Flow ---

def main():
    parser = argparse.ArgumentParser(description="Process path からゲームタイトルを推定する PoC")
    parser.add_argument("process_path", help="ゲームのプロセスパス")
    parser.add_argument("--window-title", help="補助的なウィンドウタイトル")
    parser.add_argument("--data-dir", default=r"data\vndb-db-2026-02-28\extracted\db", help="VNDB データのディレクトリ")
    args = parser.parse_args()
    
    print(f"Input Process Path: {args.process_path}")
    if args.window_title:
        print(f"Input Window Title: {args.window_title}")
        
    # Step 1: Segments
    segments = extract_segments(args.process_path)
    print(f"Step 1 Segments: {segments}")
    
    # Step 2: Tokens
    tokens = generate_tokens(segments)
    if args.window_title:
        tokens.append(args.window_title)
        
    print(f"Step 2 Tokens: {tokens}")
    
    # Step 3: Load Data and Match
    dic, official_title_map = load_vndb_dictionary(args.data_dir)
    
    all_matches = []
    print("辞書マッチングを実行中...")
    for token in tokens:
        matches = dic.match(token)
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
            print(f"  - [{det['type']}] '{det['token']}' -> '{det['matched']}' ({det['method']}, score: {det['score']})")
    else:
        print("\nタイトルを特定できませんでした。")
        if "top_candidate" in result and result["top_candidate"]:
            top = result["top_candidate"]
            print(f"最高スコア候補: {top['game_id']} (Score: {top['final_score']:.3f})")

if __name__ == "__main__":
    main()
