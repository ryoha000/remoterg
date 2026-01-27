import { sqliteTable, text } from "drizzle-orm/sqlite-core"

export const screenshotMap = sqliteTable("screenshot_map", {
  localId: text("local_id").primaryKey().notNull(),
  hostId: text("host_id").notNull(),
})
