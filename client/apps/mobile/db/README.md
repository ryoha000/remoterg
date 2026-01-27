# Database Layer Architecture

このディレクトリは、**Drizzle ORM**、**expo-sqlite**、**TanStack Query** を組み合わせた「スキーマ駆動」のデータベース実装を管理します。

## アーキテクチャ概要

3層構造で責務を分離しています。UIコンポーネントが直接DBを触ることはありません。

| 層                   | ディレクトリ   | 役割                                                                               |
| -------------------- | -------------- | ---------------------------------------------------------------------------------- |
| **Schema Layer**     | `db/schema/`   | テーブル定義（Single Source of Truth）。Drizzleの定義がそのままDB構造になります。  |
| **Repository Layer** | `db/services/` | DB操作の隠蔽。Drizzleを使ってSQLを発行する純粋な関数群。                           |
| **Hooks Layer**      | `db/queries/`  | データ取得・更新のインターフェース。UIはこのフックを通じてデータにアクセスします。 |

---

## 開発ワークフロー

### 1. テーブルの作成・変更 (Schema)

1. `db/schema/` に新しいファイルを作成するか、既存のファイルを編集してテーブルを定義します。

   ```typescript
   // db/schema/examples.ts
   import { sqliteTable, text, integer } from "drizzle-orm/sqlite-core"

   export const examples = sqliteTable("examples", {
     id: text("id").primaryKey(),
     name: text("name").notNull(),
     createdAt: integer("created_at").notNull(),
   })
   ```

2. マイグレーションファイルを生成します。

   ```bash
   pnpm drizzle-kit generate
   ```

   > これにより `db/migrations/` にSQLファイルが生成されます。アプリ起動時に自動で適用されます。

### 2. DB操作の実装 (Repository)

`db/services/` に関数を作成します。ここでは `drizzle-orm` のメソッドを使用し、**純粋な非同期関数**として実装します。HooksやReactの依存関係は含めません。

```typescript
// db/services/example-service.ts
import { db } from "../client"
import { examples } from "../schema/examples"

export const getExamples = async () => {
  return await db.select().from(examples)
}

export const addExample = async (name: string) => {
  return await db.insert(examples).values({
    id: crypto.randomUUID(),
    name,
    createdAt: Date.now(),
  })
}
```

### 3. フックの実装 (Hooks)

`db/queries/` にTanStack Queryを使ったフックを作成します。

- **読み取り**: `useQuery` を使用
- **書き込み**: `useMutation` を使用し、成功時に `invalidateQueries` でキャッシュを更新

```typescript
// db/queries/use-examples.ts
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { getExamples, addExample } from "../services/example-service"

export const useExamples = () => {
  return useQuery({
    queryKey: ["examples"],
    queryFn: getExamples,
  })
}

export const useAddExample = () => {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: addExample,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["examples"] })
    },
  })
}
```

### 4. UIでの利用

コンポーネントからはフックのみを呼び出します。

```tsx
import { useExamples, useAddExample } from "@/db/queries/use-examples";

export default function ExampleComponent() {
  const { data } = useExamples();
  const { mutate } = useAddExample();

  return (
    // ...
  );
}
```

## ルール

1. **直接SQLを書かない**: 基本的にDrizzleのビルダーを使用してください。複雑なクエリが必要な場合のみ `sql` タグの使用を検討してください。
2. **マイグレーションの手動変更禁止**: `db/migrations/` 下のファイルは自動生成されます。手動で書き換えると整合性が取れなくなります。
3. **トランザクション**: 複数の書き込みを行う場合は、必ず `db.transaction()` を使用して整合性を保ってください。
