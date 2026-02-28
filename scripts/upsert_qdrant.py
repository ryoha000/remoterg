import os
import sys
import argparse
import pandas as pd
from dotenv import load_dotenv
import torch
from sentence_transformers import SentenceTransformer
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, VectorParams, PointStruct
import gradio as gr
from tqdm import tqdm

load_dotenv()

EMBEDDING_MODEL = "Qwen/Qwen3-Embedding-4B"
COLLECTION_NAME = "vndb_titles"

def load_data(data_dir):
    print("VN データを読み込んでいます...")
    # vn を読み込む
    vn = pd.read_csv(os.path.join(data_dir, 'vn'), sep='\t', names=[
        'id', 'image', 'c_image', 'olang', 'c_votecount', 'c_rating', 'c_average', 
        'c_length', 'c_lengthnum', 'length', 'devstatus', 'alias', 'description'
    ], keep_default_na=False, dtype=str)

    # vn_titles を読み込む
    vn_titles = pd.read_csv(os.path.join(data_dir, 'vn_titles'), sep='\t', names=[
        'id', 'lang', 'official', 'title', 'latin'
    ], keep_default_na=False, dtype=str)

    # releases_vn を読み込む
    releases_vn = pd.read_csv(os.path.join(data_dir, 'releases_vn'), sep='\t', names=[
        'id', 'vid', 'rtype'
    ], keep_default_na=False, dtype=str)

    # releases_producers を読み込む
    releases_producers = pd.read_csv(os.path.join(data_dir, 'releases_producers'), sep='\t', names=[
        'id', 'pid', 'developer', 'publisher'
    ], keep_default_na=False, dtype=str)

    # producers を読み込む
    producers = pd.read_csv(os.path.join(data_dir, 'producers'), sep='\t', names=[
        'id', 'type', 'lang', 'name', 'latin', 'alias', 'description'
    ], keep_default_na=False, dtype=str)

    return vn, vn_titles, releases_vn, releases_producers, producers

def extract_metadata(vn, vn_titles, releases_vn, releases_producers, producers):
    print("タイトルを抽出しています...")
    # 優先順位: 1. 日本語(ja)かつofficial='t'  2. 任意の日本語(ja)  3. 任意のofficial='t'  4. 最初に見つかったもの
    title_ja_official = vn_titles[(vn_titles['lang'] == 'ja') & (vn_titles['official'] == 't')].groupby('id')['title'].first().reset_index()
    title_ja = vn_titles[vn_titles['lang'] == 'ja'].groupby('id')['title'].first().reset_index()
    title_official = vn_titles[vn_titles['official'] == 't'].groupby('id')['title'].first().reset_index()
    title_any = vn_titles.groupby('id')['title'].first().reset_index()

    # まず all_titles をベースにする
    titles = title_any.rename(columns={'title': 'any_title'})
    
    # 次に official をマージ
    titles = pd.merge(titles, title_official.rename(columns={'title': 'official_title'}), on='id', how='left')
    
    # 次に ja をマージ
    titles = pd.merge(titles, title_ja.rename(columns={'title': 'ja_title'}), on='id', how='left')
    
    # 最後に ja_official をマージ
    titles = pd.merge(titles, title_ja_official.rename(columns={'title': 'ja_official_title'}), on='id', how='left')

    # 優先順位に従って 'official_title' カラムに値を入れる
    titles['best_title'] = titles['ja_official_title'].fillna(
        titles['ja_title'].fillna(
            titles['official_title'].fillna(titles['any_title'])
        )
    )
    
    titles.drop(columns=['any_title', 'official_title', 'ja_title', 'ja_official_title'], inplace=True)
    titles.rename(columns={'best_title': 'official_title'}, inplace=True)
    
    # すべてのタイトル（別言語、ラテン文字表記含む）をまとめる
    all_t = pd.concat([
        vn_titles[['id', 'title']].rename(columns={'title': 't'}),
        vn_titles[['id', 'latin']].rename(columns={'latin': 't'})
    ])
    all_t = all_t[all_t['t'].astype(bool) & (all_t['t'].str.strip() != '') & (all_t['t'] != '\\N')]
    all_titles_df = all_t.groupby('id')['t'].apply(lambda x: ' '.join(set(x))).reset_index(name='all_titles')
    
    titles = pd.merge(titles, all_titles_df, on='id', how='left')
    
    print("制作会社を抽出しています...")
    # 各VNのデベロッパーを取得
    rel_prod = pd.merge(releases_vn, releases_producers, on='id')
    developers = rel_prod[rel_prod['developer'] == 't']
    dev_names = pd.merge(developers, producers, left_on='pid', right_on='id')
    
    # ブランドのすべての名前（name, latin, alias）をまとめる関数
    def get_all_brand_names(df):
        names = []
        for col in ['name', 'latin', 'alias']:
            # NaN や '\N' を除外し、改行をスペースにして分割
            vals = df[col].astype(str).replace(r'\\N|nan', '', regex=True).replace(r'\\n|\n', ' ', regex=True).str.strip()
            for v in vals:
                if v:
                    # スペース区切りの単語やカンマを考慮せず、そのままの文字列セットとして扱うが、
                    # 複数登録されているエイリアスなどはスペースで区切られている可能性がある
                    names.append(v)
        # 一意にしてスペース結合
        return ' '.join(set([n for n in names if n]))

    vn_devs = dev_names.groupby('vid').apply(get_all_brand_names).reset_index(name='brand_name')
    vn_devs.rename(columns={'vid': 'id'}, inplace=True)

    
    print("メタデータをマージしています...")
    meta = pd.merge(vn[['id', 'alias', 'description']], titles, on='id', how='left')
    meta = pd.merge(meta, vn_devs, on='id', how='left')
    
    meta['brand_name'] = meta['brand_name'].fillna('')
    meta['official_title'] = meta['official_title'].fillna('')
    meta['all_titles'] = meta['all_titles'].fillna('')
    meta['description'] = meta['description'].fillna('')
    meta['alias'] = meta['alias'].fillna('').replace('\\N', '')
    meta['alias'] = meta['alias'].str.replace(r'\\n|\n', ' ', regex=True)
    
    return meta

def get_embeddings(texts, model, batch_size=16):
    """SentenceTransformersを使用してテキストの埋め込みベクトルを取得する"""
    embeddings = model.encode(texts, batch_size=batch_size, convert_to_numpy=True, show_progress_bar=False)
    return embeddings.tolist()

def setup_db(client, dim):
    # コレクション作成（存在しなければ）
    if not client.collection_exists(COLLECTION_NAME):
        client.create_collection(
            collection_name=COLLECTION_NAME,
            vectors_config=VectorParams(size=dim, distance=Distance.COSINE),
        )

def upsert_data(meta_df, model, client, dry_run=False):
    dim = model.get_sentence_embedding_dimension()
    setup_db(client, dim)
    
    batch_size = 16 
    records = meta_df.to_dict('records')
    
    progress_file = "qdrant_upsert_progress.txt"
    start_idx = 0
    if os.path.exists(progress_file) and not dry_run:
        with open(progress_file, "r") as f:
            try:
                start_idx = int(f.read().strip())
                print(f"前回の進捗ファイルが見つかりました: {start_idx} 件目から再開します。")
            except ValueError:
                print("進捗ファイルの読み出しに失敗しました。最初から開始します。")
    
    if dry_run:
        print("\n--- ドライラン実行 ---")
        print(f"以下のようなデータが Qdrant ({EMBEDDING_MODEL}, 次元数: {dim}) にアップサートされます：\n")
        sample_batch = records[:3]
        
        sample_texts = [
            f"{r['all_titles']} {r['alias']} {r['brand_name']}".replace('\n', ' ').strip()
            for r in sample_batch
        ]
        
        print("埋め込みベクトルを生成しています...")
        sample_embeddings = get_embeddings(sample_texts, model, batch_size=batch_size)
        
        for r, emb in zip(sample_batch, sample_embeddings):
            try:
                numeric_id = int(str(r['id']).lstrip('v'))
            except ValueError:
                numeric_id = hash(str(r['id'])) & ((1<<63)-1)
                
            print(f"ID (String): {r['id']} -> (Numeric): {numeric_id}")
            print(f"Metadata: {{'game_id': '{r['id']}', 'official_title': '{r['official_title']}', 'brand_name': '{r['brand_name']}'}}")
            print(f"Text for embedding: {sample_texts[sample_batch.index(r)]!r}")
            print(f"Vector preview: [{emb[0]:.4f}, {emb[1]:.4f}, ..., {emb[-1]:.4f}] (次元数: {len(emb)})\n")
            
        print("ドライランが完了しました。実際のアップサートは行われていません。")
        return

    print(f"全 {len(records)} 件のレコードを {batch_size} 件ずつアップサートします... (Ctrl+C で中断可能)")
    
    try:
        for i in tqdm(range(start_idx, len(records), batch_size), initial=start_idx//batch_size, total=(len(records) + batch_size - 1)//batch_size):
            batch = records[i:i+batch_size]
            
            texts = [
                f"{r['all_titles']} {r['alias']} {r['brand_name']}".replace('\n', ' ').strip()
                for r in batch
            ]
            
            try:
                # `get_embeddings` 内でも指定したバッチサイズで効率的にエンコード
                embeddings = get_embeddings(texts, model, batch_size=len(texts))
            except Exception as e:
                print(f"埋め込み生成中にエラーが発生しました (batch starting at index {i}): {e}")
                continue
            
            points = []
            for r, emb in zip(batch, embeddings):
                # QdrantのID用
                try:
                    numeric_id = int(str(r['id']).lstrip('v'))
                except ValueError:
                    numeric_id = hash(str(r['id'])) & ((1<<63)-1)
                    
                points.append(PointStruct(
                    id=numeric_id,
                    vector=emb,
                    payload={
                        'game_id': str(r['id']),
                        'official_title': str(r['official_title']),
                        'brand_name': str(r['brand_name']),
                    }
                ))
            
            client.upsert(collection_name=COLLECTION_NAME, points=points)
            
            # 進捗を保存
            with open(progress_file, "w") as f:
                f.write(str(i + len(batch)))
            
    except KeyboardInterrupt:
        print(f"\n中断されました。次回は {i} 件目から再開できます。")
        return
        
    print("アップサート完了！ http://localhost:6333/dashboard で確認できます")
    if os.path.exists(progress_file):
        os.remove(progress_file)

def launch_ui(model, client):
    def search(query):
        if not query.strip():
            return "クエリを入力してください。"
            
        query_vector = model.encode(query, convert_to_numpy=True).tolist()
        results = client.query_points(
            collection_name=COLLECTION_NAME,
            query=query_vector,
            limit=5
        ).points
        
        output = ""
        for res in results:
            p = res.payload
            output += f"### {p['official_title']} (Score: {res.score:.4f})\n"
            output += f"**Brand:** {p['brand_name']}  \n"
            output += f"**ID:** {p.get('game_id', '')}  \n"
            
        if not output:
            output = "結果が見つかりませんでした。"
        return output

    with gr.Blocks() as demo:
        gr.Markdown(f"# {EMBEDDING_MODEL} ベクトル検索デモ (Qdrant)")
        with gr.Row():
            query_input = gr.Textbox(label="検索クエリ", placeholder="例：泣けるエロゲー...", scale=4)
            search_button = gr.Button("検索", scale=1)
        output_display = gr.Markdown()
        
        search_button.click(search, inputs=query_input, outputs=output_display)
        query_input.submit(search, inputs=query_input, outputs=output_display)
    
    print("Gradio UIを起動します... Webブラウザで http://127.0.0.1:7860 にアクセスしてください。")
    demo.launch(server_name="0.0.0.0", server_port=7860)

def main():
    parser = argparse.ArgumentParser(description="VNDBのデータを解析し、QdrantにアップサートしてUIを起動します。")
    parser.add_argument("--dry-run", action="store_true", help="実際のアップサートを行わず、サンプルの出力のみを表示します。")
    parser.add_argument("--skip-upsert", action="store_true", help="アップサートをスキップして、UIのみを起動します。")
    args = parser.parse_args()

    print(f"モデル '{EMBEDDING_MODEL}' を読み込んでいます...")
    device = "cuda" if torch.cuda.is_available() else "cpu"
    
    if device == "cpu":
        print("エラー: GPU (CUDA) が利用できません。強制終了します。")
        print("ヒント: 'uv run' で pytorch-cu121 が正しくダウンロードされているか確認してください。")
        sys.exit(1)
    
    print("✅ GPU (CUDA) の利用が確認できました。")
    # 4Bモデルを16GB VRAMに収めるためfloat16を使用
    model_kwargs = {"torch_dtype": torch.float16} if device == "cuda" else {}
    model = SentenceTransformer(
        EMBEDDING_MODEL, 
        model_kwargs=model_kwargs,
        trust_remote_code=True
    )
    
    print("ローカルのQdrantに接続しています (localhost:6333)...")
    client = QdrantClient("localhost", port=6333)

    if not args.skip_upsert:
        data_dir = r"data\vndb-db-2026-02-28\extracted\db"
        if not os.path.exists(data_dir):
            print(f"エラー: データディレクトリが見つかりません: {data_dir}")
            print("VNDBデータをダウンロード・展開しているか確認してください。")
            return
            
        vn, vn_titles, releases_vn, releases_producers, producers = load_data(data_dir)
        meta = extract_metadata(vn, vn_titles, releases_vn, releases_producers, producers)
        upsert_data(meta, model, client, dry_run=args.dry_run)
        
    if not args.dry_run:
        launch_ui(model, client)

if __name__ == "__main__":
    main()
