import sys
import argparse
import json
from identify_title import (
    extract_segments, 
    generate_tokens, 
    load_vndb_dictionary, 
    score_and_select
)

def main():
    parser = argparse.ArgumentParser(description="VNDB 辞書を保持して入力を待ち受ける REPL")
    parser.add_argument("--data-dir", default=r"data\vndb-db-2026-02-28\extracted\db", help="VNDB データのディレクトリ")
    parser.add_argument("--json", action="store_true", help="JSON 形式で出力")
    args = parser.parse_args()

    # Load dictionary once
    dic, official_title_map = load_vndb_dictionary(args.data_dir)
    
    print("\n--- REPL Ready. Enter process path (and optional window title, pipe-separated) ---")
    print("Example: G:\\game\\path\\to.exe | Window Title")
    print("Enter 'exit' or 'q' to quit.\n")

    while True:
        try:
            line = input("> ").strip()
            if not line:
                continue
            if line.lower() in ("exit", "quit", "q"):
                break
                
            # Support "path | window_title" format
            if "|" in line:
                path, window_title = [part.strip() for part in line.split("|", 1)]
            else:
                path = line
                window_title = None
                
            # Process
            segments = extract_segments(path)
            tokens = generate_tokens(segments)
            if window_title:
                tokens.append(window_title)
            
            all_matches = []
            for token in tokens:
                matches = dic.match(token)
                all_matches.extend(matches)
                
            result = score_and_select(all_matches, official_title_map)
            
            if args.json:
                print(json.dumps(result, ensure_ascii=False, indent=2))
            else:
                if result["identified"]:
                    print(f"Result: {result['game_id']} [{result['official_title']}] (Confidence: {result['confidence']:.3f})")
                    for det in result["match_details"][:3]: # Show top 3 details
                         print(f"  - [{det['type']}] '{det['token']}' -> '{det['matched']}' ({det['method']})")
                else:
                    print("Not identified.")
                    
        except KeyboardInterrupt:
            break
        except Exception as e:
            print(f"Error: {e}")

    print("Goodbye!")

if __name__ == "__main__":
    main()
