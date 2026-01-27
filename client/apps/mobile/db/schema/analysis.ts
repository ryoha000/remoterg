import { sqliteTable, text, integer } from "drizzle-orm/sqlite-core"

export const analysisResults = sqliteTable("analysis_results", {
  localId: text("local_id").primaryKey().notNull(),
  data: text("data").notNull(), // Storing JSON as text
  createdAt: integer("created_at").notNull(),
})
