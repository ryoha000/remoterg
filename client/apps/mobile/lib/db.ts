import * as SQLite from "expo-sqlite"

import { type AnalysisResult } from "@/features/viewer/types"

// Database name
const DB_NAME = "remoterg.db"

// Table schema
// screenshot_map: Link local asset ID with host UUID
// analysis_results: Store analysis keyed by local asset ID
const CREATE_TABLES_QUERY = `
  CREATE TABLE IF NOT EXISTS screenshot_map (
    local_id TEXT PRIMARY KEY NOT NULL,
    host_id TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS analysis_results (
    local_id TEXT PRIMARY KEY NOT NULL,
    data TEXT NOT NULL,
    created_at INTEGER NOT NULL
  );
`

let db: SQLite.SQLiteDatabase | null = null

// Initialize database
export const initDatabase = async () => {
  if (db) return

  try {
    db = await SQLite.openDatabaseAsync(DB_NAME)
    await db.execAsync(CREATE_TABLES_QUERY)
    console.log("Database initialized successfully")
  } catch (error) {
    console.error("Failed to initialize database", error)
    throw error
  }
}

// Map screenshot IDs
export const mapScreenshot = async (localId: string, hostId: string) => {
  if (!db) await initDatabase()
  if (!db) throw new Error("Database not initialized")

  try {
    await db.runAsync(
      "INSERT OR REPLACE INTO screenshot_map (local_id, host_id) VALUES (?, ?);",
      localId,
      hostId,
    )
    console.log(`Mapped ${localId} -> ${hostId}`)
  } catch (error) {
    console.error(`Failed to map screenshot ${localId} -> ${hostId}`, error)
  }
}

// Get host ID from local ID
export const getHostId = async (localId: string): Promise<string | null> => {
  if (!db) await initDatabase()
  if (!db) throw new Error("Database not initialized")

  try {
    const result = await db.getFirstAsync<{ host_id: string }>(
      "SELECT host_id FROM screenshot_map WHERE local_id = ?;",
      localId,
    )
    return result?.host_id || null
  } catch (error) {
    console.error(`Failed to get host ID for ${localId}`, error)
    return null
  }
}

// Get local IDs from host ID (could be multiple if re-downloaded, though unlikely with our flow)
export const getLocalIds = async (hostId: string): Promise<string[]> => {
  if (!db) await initDatabase()
  if (!db) throw new Error("Database not initialized")

  try {
    const results = await db.getAllAsync<{ local_id: string }>(
      "SELECT local_id FROM screenshot_map WHERE host_id = ?;",
      hostId,
    )
    return results.map((r) => r.local_id)
  } catch (error) {
    console.error(`Failed to get local IDs for ${hostId}`, error)
    return []
  }
}

// Save analysis result (keyed by local ID)
export const saveAnalysis = async (localId: string, analysis: AnalysisResult) => {
  if (!db) await initDatabase()

  if (!db) throw new Error("Database not initialized")

  try {
    const json = JSON.stringify(analysis)
    const createdAt = Date.now()

    await db.runAsync(
      "INSERT OR REPLACE INTO analysis_results (local_id, data, created_at) VALUES (?, ?, ?);",
      localId,
      json,
      createdAt,
    )
    console.log(`Saved analysis for local ID: ${localId}`)
  } catch (error) {
    console.error(`Failed to save analysis for ${localId}`, error)
  }
}

// Get analysis result by local ID
export const getAnalysis = async (localId: string): Promise<AnalysisResult | null> => {
  if (!db) await initDatabase()

  if (!db) throw new Error("Database not initialized")

  try {
    const result = await db.getFirstAsync<{ data: string }>(
      "SELECT data FROM analysis_results WHERE local_id = ?;",
      localId,
    )

    if (result) {
      return JSON.parse(result.data) as AnalysisResult
    }
    return null
  } catch (error) {
    console.error(`Failed to get analysis for ${localId}`, error)
    return null
  }
}
