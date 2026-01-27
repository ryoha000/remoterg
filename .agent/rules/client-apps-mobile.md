---
trigger: glob
globs: client/apps/mobile/**
---

# UIコンポーネントの追加

コンポーネント実装時に汎用的なコンポーネントが必要な時は実装するより先に react-native-reusables に既に存在していないか検討してください。

react-native-reusables からは client/apps/mobile で以下のコマンドを実行することでUIコンポーネントを追加できます。(ボタンコンポーネントの場合)
```
pnpm dlx @react-native-reusables/cli@latest add button
```

# コード編集後

コードを編集してユーザーに提出する前には必ず `pnpm fmt` を実行してフォーマットをかけてください。
