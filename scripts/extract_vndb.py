import zstandard as zstd
import tarfile
import os
import sys

def extract_tar_zst(archive_path, extract_path):
    print(f"Extracting {archive_path} to {extract_path}...")
    if not os.path.exists(extract_path):
        os.makedirs(extract_path)
    
    dctx = zstd.ZstdDecompressor()
    with open(archive_path, 'rb') as ifh:
        with dctx.stream_reader(ifh) as reader:
            with tarfile.open(fileobj=reader, mode='r|') as tar:
                tar.extractall(path=extract_path)
    print("Extraction complete.")

if __name__ == "__main__":
    archive = r"scripts\data\vndb-db-2026-02-28\vndb-db-2026-02-28.tar.zst"
    output_dir = r"scripts\data\vndb-db-2026-02-28\extracted"
    extract_tar_zst(archive, output_dir)
