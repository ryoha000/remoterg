import os
import argparse
import pandas as pd
from dotenv import load_dotenv
from pinecone import Pinecone, ServerlessSpec
import torch
from sentence_transformers import SentenceTransformer
from tqdm import tqdm

load_dotenv()

PINECONE_API_KEY = os.getenv("PINECONE_API_KEY")
INDEX_NAME = os.getenv("PINECONE_INDEX_NAME", "vndb-titles")
EMBEDDING_MODEL = "Qwen/Qwen3-Embedding-4B"

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
    # それぞれの条件でフィルタリング
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
    
    print("制作会社を抽出しています...")
    # 各VNのデベロッパーを取得
    rel_prod = pd.merge(releases_vn, releases_producers, on='id')
    developers = rel_prod[rel_prod['developer'] == 't']
    dev_names = pd.merge(developers, producers, left_on='pid', right_on='id')
    
    vn_devs = dev_names.groupby('vid')['name'].apply(lambda x: ', '.join(x.unique())).reset_index()
    vn_devs.rename(columns={'vid': 'id', 'name': 'brand_name'}, inplace=True)
    
    print("メタデータをマージしています...")
    meta = pd.merge(vn[['id', 'alias', 'description']], titles, on='id', how='left')
    meta = pd.merge(meta, vn_devs, on='id', how='left')
    
    meta['brand_name'] = meta['brand_name'].fillna('')
    meta['official_title'] = meta['official_title'].fillna('')
    meta['description'] = meta['description'].fillna('')
    
    return meta

def get_embeddings(texts, model):
    """SentenceTransformersを使用してテキストの埋め込みベクトルを取得する"""
    # 16GB VRAMに収まるようバッチサイズは適宜調整 (ここでは encode のデフォルトを使用)
    embeddings = model.encode(texts, convert_to_numpy=True, show_progress_bar=False)
    return embeddings.tolist()

def batch_upsert(meta_df, dry_run=False):
    print(f"モデル '{EMBEDDING_MODEL}' を読み込んでいます...")
    device = "cuda" if torch.cuda.is_available() else "cpu"
    # 4Bモデルを16GB VRAMに収めるためfloat16を使用
    model_kwargs = {"torch_dtype": torch.float16} if device == "cuda" else {}
    
    model = SentenceTransformer(
        EMBEDDING_MODEL, 
        model_kwargs=model_kwargs,
        trust_remote_code=True
    )
    DIMENSION = model.get_sentence_embedding_dimension()
    
    # 4Bモデルの場合VRAMを考慮してバッチサイズを小さめに設定
    batch_size = 16 
    records = meta_df.to_dict('records')
    
    if dry_run:
        print("\n--- ドライラン実行 ---")
        print(f"以下のようなデータが Pinecone ({EMBEDDING_MODEL}, 次元数: {DIMENSION}) にアップサートされます：\n")
        sample_batch = records[:3]
        
        sample_texts = [
            f"{r['official_title']} {r['brand_name']}"
            for r in sample_batch
        ]
        
        print("ローカルモデルで埋め込みベクトルを生成しています...")
        sample_embeddings = get_embeddings(sample_texts, model)
        
        for r, emb in zip(sample_batch, sample_embeddings):
            sample_vector = {
                'id': str(r['id']),
                'values': f"[{emb[0]:.4f}, {emb[1]:.4f}, ..., {emb[-1]:.4f}] (次元数: {len(emb)})",
                'metadata': {
                    'game_id': str(r['id']),
                    'official_title': str(r['official_title']),
                    'brand_name': str(r['brand_name']),
                }
            }
            print(f"ID: {sample_vector['id']}")
            print(f"Metadata: {sample_vector['metadata']}")
            print(f"Text for embedding: {sample_texts[sample_batch.index(r)]!r}")
            print(f"Vector preview: {sample_vector['values']}\n")
            
        print("ドライランが完了しました。実際のアップサートは行われていません。")
        return

    if not PINECONE_API_KEY:
        print("エラー: .env に PINECONE_API_KEY が設定されていません")
        return

    print("Pinecone を初期化しています...")
    pc = Pinecone(api_key=PINECONE_API_KEY)
    
    if INDEX_NAME not in pc.list_indexes().names():
        print(f"インデックス '{INDEX_NAME}' ({DIMENSION}次元) を作成しています...")
        pc.create_index(
            name=INDEX_NAME, 
            dimension=DIMENSION, 
            metric='cosine',
            spec=ServerlessSpec(
                cloud='aws',
                region='us-east-1'
            )
        )
    
    idx = pc.Index(INDEX_NAME)
    
    print(f"{len(records)} 件のレコードを {batch_size} 件ずつアップサートします...")
    for i in tqdm(range(0, len(records), batch_size)):
        batch = records[i:i+batch_size]
        
        sample_texts = [
            f"{r['official_title']} {r['brand_name']}"
            for r in sample_batch
        ]
        
        try:
            embeddings = get_embeddings(texts, model)
        except Exception as e:
            print(f"埋め込み生成中にエラーが発生しました (batch starting at index {i}): {e}")
            continue
        
        vectors = []
        for r, emb in zip(batch, embeddings):
            vectors.append({
                'id': str(r['id']),
                'values': emb,
                'metadata': {
                    'game_id': str(r['id']),
                    'official_title': str(r['official_title']),
                    'brand_name': str(r['brand_name']),
                }
            })
        
        idx.upsert(vectors=vectors)
        
    print("アップサートが完了しました。")

def main():
    parser = argparse.ArgumentParser(description="VNDBのデータを解析し、Pineconeにアップサートします。")
    parser.add_argument("--dry-run", action="store_true", help="実際のアップサートを行わず、サンプルの出力のみを表示します。")
    args = parser.parse_args()

    data_dir = r"data\vndb-db-2026-02-28\extracted\db"
    vn, vn_titles, releases_vn, releases_producers, producers = load_data(data_dir)
    meta = extract_metadata(vn, vn_titles, releases_vn, releases_producers, producers)
    batch_upsert(meta, dry_run=args.dry_run)

if __name__ == "__main__":
    main()
