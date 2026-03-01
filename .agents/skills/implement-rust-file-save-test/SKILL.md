---
name: implement-rust-file-save-test
description: Implement a Rust program that saves files to disk and tests the file saving functionality.
---

## テスト時のファイル保存について

Rustのプログラムでファイル保存機能などのテストを実装する際、ハードコードされた仮のパス（`"dummy"` など）を使用せず、必ず `std::env::temp_dir()` を活用してテスト用の一時ディレクトリを利用すること。

また、テスト終了時には必ずクリーンアップを行い、作成した一時ファイルやディレクトリを `tokio::fs::remove_dir_all` などを利用して削除するように実装すること。

### 実装例

```rust
#[tokio::test]
async fn test_save_file() {
    let temp_dir = std::env::temp_dir().join("my_app_test_dir");
    
    // 一時ディレクトリを作成
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    // テスト対象の処理（temp_dir 以下にファイルを保存するなど）
    // ...
    
    // クリーンアップ
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
```
