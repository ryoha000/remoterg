import { sqliteTable, text } from "drizzle-orm/sqlite-core"

export const screenshotFavorites = sqliteTable("screenshot_favorites", {
  localId: text("local_id").primaryKey().notNull(),
})
