import os
import shutil
import argparse
from pathlib import Path

# uvx などの独立環境で実行する場合は以下のコメントブロックによって依存関係が解決されます
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "huggingface-hub",
#     "ultralytics",
#     "onnx",
#     "torch",
# ]
# ///

def export_yolo():
    print("=== YOLOv8 Animeface の ONNX エクスポートを開始します ===")
    from huggingface_hub import hf_hub_download
    from ultralytics import YOLO
    
    # Hugging Face からモデルをダウンロード
    hf_path = hf_hub_download(repo_id="Fuyucchi/yolov8_animeface", filename="yolov8x6_animeface.pt")
    local_path = "yolov8x6_animeface.pt"
    
    # ローカルにコピー (ultralytics の export はモデルのディレクトリに出力するため)
    if not os.path.exists(local_path):
        shutil.copy(hf_path, local_path)
        
    print(f"モデルを読み込んでいます: {local_path}")
    model = YOLO(local_path)
    
    # ONNX へエクスポート
    # dynamic=True にすることで、推論時の画像サイズを可変にすることができます。
    print("ONNX 形式にエクスポートしています...")
    export_path = model.export(format="onnx", opset=14, dynamic=True)
    print(f"YOLOv8 のエクスポートが完了しました: {export_path}")
    print()

def export_dinov2():
    print("=== DINOv2 ViT-S/14 の ONNX エクスポートを開始します ===")
    import torch
    
    # PyTorch Hub から DINOv2 をロード
    print("DINOv2 モデルをロードしています...")
    model = torch.hub.load('facebookresearch/dinov2', 'dinov2_vits14')
    model.eval()
    
    # ViT のパッチサイズは 14 なので、14の倍数である必要があります。標準的な 224x224 をダミー入力とする。
    dummy_input = torch.randn(1, 3, 224, 224)
    output_path = "dinov2_vits14.onnx"
    
    print("ONNX 形式にエクスポートしています...")
    torch.onnx.export(
        model,
        dummy_input,
        output_path,
        export_params=True,
        opset_version=14,
        do_constant_folding=True,
        input_names=['input'],
        output_names=['output'],
        dynamic_axes={
            'input': {0: 'batch_size', 2: 'height', 3: 'width'},
            'output': {0: 'batch_size'}
        }
    )
    print(f"DINOv2 のエクスポートが完了しました: {output_path}")
    print()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="指定されたモデル(YOLOv8, DINOv2)をONNXにエクスポートするスクリプト")
    parser.add_argument("--yolo", action="store_true", help="YOLOv8 Animeface のみをエクスポートする")
    parser.add_argument("--dinov2", action="store_true", help="DINOv2 のみをエクスポートする")
    args = parser.parse_args()
    
    if not args.yolo and not args.dinov2:
        # 両方実行
        export_yolo()
        export_dinov2()
    else:
        if args.yolo:
            export_yolo()
        if args.dinov2:
            export_dinov2()
