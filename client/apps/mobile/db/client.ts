import { drizzle } from "drizzle-orm/expo-sqlite"
import { useMigrations } from "drizzle-orm/expo-sqlite/migrator"
import { openDatabaseSync } from "expo-sqlite"

import migrations from "./migrations/migrations"

const DB_NAME = "remoterg.db"
const expoDb = openDatabaseSync(DB_NAME)

export const db = drizzle(expoDb)

export const useMigration = () => {
  return useMigrations(db, migrations)
}
