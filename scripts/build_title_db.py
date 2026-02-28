import os
import sys
import sqlite3
import argparse
from datetime import datetime

# identify_title から関連関数をインポート
from identify_title import load_vndb_dictionary, normalize_for_match

def build_sqlite_db(data_dir: str, output_path: str):
    print(f"Loading data from {data_dir}...")
    dic, official_title_map = load_vndb_dictionary(data_dir)

    print(f"Creating SQLite database at {output_path}...")
    if os.path.exists(output_path):
        os.remove(output_path)

    conn = sqlite3.connect(output_path)
    cursor = conn.cursor()

    # テーブル作成
    cursor.executescript("""
        CREATE TABLE dict_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            normalized_name TEXT NOT NULL,
            no_space_name TEXT NOT NULL,
            game_id TEXT NOT NULL,
            match_type TEXT NOT NULL,
            original_name TEXT NOT NULL
        );
        CREATE INDEX idx_normalized ON dict_entries(normalized_name);
        CREATE INDEX idx_no_space ON dict_entries(no_space_name);
        CREATE INDEX idx_game_id ON dict_entries(game_id);

        CREATE TABLE games (
            game_id TEXT PRIMARY KEY,
            official_title TEXT NOT NULL
        );

        CREATE TABLE metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
    """)

    # games テーブルへの挿入
    print("Inserting games...")
    games_data = [(game_id, title) for game_id, title in official_title_map.items()]
    cursor.executemany("INSERT INTO games (game_id, official_title) VALUES (?, ?)", games_data)

    # dict_entries テーブルへの挿入
    print("Inserting dict_entries...")
    entries_data = []
    for normalized_name, list_info in dic.entries.items():
        no_space_name = normalized_name.replace(" ", "")
        for game_id, match_type, original_name in list_info:
            entries_data.append((normalized_name, no_space_name, game_id, match_type, original_name))

    # バッチインサートで高速化
    cursor.executemany("""
        INSERT INTO dict_entries (normalized_name, no_space_name, game_id, match_type, original_name)
        VALUES (?, ?, ?, ?, ?)
    """, entries_data)

    # metadata テーブルへの挿入
    print("Inserting metadata...")
    today = datetime.now().strftime('%Y-%m-%d')
    entry_count = len(entries_data)
    cursor.execute("INSERT INTO metadata (key, value) VALUES ('version', ?)", (today,))
    cursor.execute("INSERT INTO metadata (key, value) VALUES ('entry_count', ?)", (str(entry_count),))

    conn.commit()
    conn.close()

    print(f"Done! Inserted {len(games_data)} games and {entry_count} dictionary entries.")

def main():
    parser = argparse.ArgumentParser(description="VNDBデータから見出し検索用SQLite辞書を構築する")
    parser.add_argument("--data-dir", default="db", help="VNDB データのディレクトリ (デフォルト: db)")
    parser.add_argument("--output", default="vndb_titles.db", help="出力先 SQLite DB のパス (デフォルト: vndb_titles.db)")
    args = parser.parse_args()

    build_sqlite_db(args.data_dir, args.output)

if __name__ == "__main__":
    main()
