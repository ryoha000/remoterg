import argparse
import os
import sys
import pandas as pd

# 同じディレクトリのモジュールから再利用
from upsert_qdrant import load_data, extract_metadata
from identify_title import extract_segments, generate_tokens, load_vndb_dictionary, score_and_select

def main():
    parser = argparse.ArgumentParser(description="対話的にパスを入力し、タイトル推定ロジックのトークン生成と詳細なスコア結果をプレビューします。")
    parser.add_argument("--data-dir", default=r"data\vndb-db-2026-02-28\extracted\db", help="VNDB データのディレクトリ")
    args = parser.parse_args()

    if not os.path.exists(args.data_dir):
        print(f"エラー: データディレクトリが見つかりません: {args.data_dir}")
        print("VNDBデータをダウンロード・展開しているか確認してください。")
        sys.exit(1)
        
    print(f"\nデータの読み込みと辞書構築を開始します...")
    dic, official_title_map = load_vndb_dictionary(args.data_dir)

    print("\n準備が完了しました。")
    print("評価したいプロセスパス（例: G:\\game\\...\\xxx.exe）を入力してください。('exit' または 'quit' で終了)")

    while True:
        try:
            query = input("\n> プロセスパス: ").strip()
        except (EOFError, KeyboardInterrupt):
            print("\n終了します。")
            break
            
        if not query:
            continue
            
        if query.lower() in ['exit', 'quit']:
            print("終了します。")
            break
            
        print("\n--- [Step 1] パスセグメント抽出 ---")
        segments = extract_segments(query)
        print(f"抽出結果: {segments}")
        if not segments:
            print("有効なセグメントが見つかりませんでした。別のパスを試してください。")
            continue
            
        print("\n--- [Step 2] トークン生成 ---")
        tokens = generate_tokens(segments)
        print(f"生成トークン一覧 ({len(tokens)}件):")
        for tok, rule in tokens:
            t_label = f"'{tok}' ({rule})"
            print(f"  - {t_label}")
            
        print("\n--- [Step 3] 辞書マッチング ---")
        all_matches = []
        for token_str, rule in tokens:
            matches = dic.match(token_str)
            for m in matches:
                m.derived_rule = rule
            all_matches.extend(matches)
            
        if not all_matches:
            print("どのトークンも辞書にマッチしませんでした。")
            continue
            
        print(f"合計 {len(all_matches)} 件の辞書ヒットがありました。")
        
        print("\n--- [Step 4] スコアリングと選定 ---")
        result = score_and_select(all_matches, official_title_map)
        
        if result["identified"]:
            print(f"\n最終判定結果: ========> 【 同定成功 】 <========")
            print(f"Game ID: {result['game_id']}")
            print(f"Official Title: {result['official_title']}")
            print(f"Confidence (Final Score): {result['confidence']:.3f}\n")
            
            print("【 スコア詳細 (上位マッチ抜粋) 】")
            # 重複を整理して表示
            detailed_matches = result["match_details"]
            detailed_matches.sort(key=lambda x: x['score'], reverse=True)
            
            # 最大20件程度表示
            display_limit = 20
            for idx, det in enumerate(detailed_matches):
                if idx >= display_limit:
                    print(f"  ...他 {len(detailed_matches) - display_limit}件 (省略)")
                    break
                print(f"  [{det['score']:.3f}] Type: {det['type']:<15} | Match: {det['method']:<25} | Token: '{det['token']}' ({det['rule']}) -> Dict: '{det['matched']}'")
        else:
            print(f"\n最終判定結果: ========> 【 同定失敗（閾値 0.4 未満）】 <========")
            if "top_candidate" in result and result["top_candidate"]:
                top = result["top_candidate"]
                print(f"最高スコアだった候補 (ID: {top['game_id']}): {official_title_map.get(top['game_id'], 'Unknown')}")
                print(f"最高スコア: {top['final_score']:.3f}")
                
        # その他の候補の表示 (2位〜)
        print("\n--- 次点候補 (参考) ---")
        
        # calculate all grouped totals to show other competitors
        grouped = {}
        for m in all_matches:
            if m.game_id not in grouped: grouped[m.game_id] = []
            grouped[m.game_id].append(m)
            
        competitors = []
        for gid, gmatches in grouped.items():
            best = max(gmatches, key=lambda x: x.weighted_score)
            has_title = any(m.match_type.startswith("title") or m.match_type == "alias" for m in gmatches)
            has_brand = any(m.match_type.startswith("brand") for m in gmatches)
            cross_bonus = 0.1 if (has_title and has_brand) else 0.0
            competitors.append({
                "id": gid, 
                "score": best.weighted_score + cross_bonus,
                "best_method": best.match_method,
                "best_token": best.token,
                "best_token_rule": best.derived_rule,
                "best_matched": best.matched_name,
                "match_count": len(gmatches)
            })
            
        competitors.sort(key=lambda x: x["score"], reverse=True)
        # Skip the winner if successfully identified
        start_idx = 1 if result["identified"] and competitors and competitors[0]["id"] == result["game_id"] else 0
        for i, comp in enumerate(competitors[start_idx:start_idx+5], 1):
            title = official_title_map.get(comp['id'], 'Unknown')
            print(f" {i}位: [{comp['score']:.3f}] {title} (ID: {comp['id']}) - Hits: {comp['match_count']}")
            print(f"      Best match: Token '{comp['best_token']}' ({comp['best_token_rule']}) -> Dict '{comp['best_matched']}' ({comp['best_method']})")

if __name__ == "__main__":
    main()
