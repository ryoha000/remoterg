import { eq, desc, sql } from "drizzle-orm"

import { db } from "../client"
import { screenshotMap } from "../schema/screenshots"

export const mapScreenshot = async (
  localId: string,
  hostId: string,
  metadata?: { windowTitle?: string; processPath?: string; processName?: string },
) => {
  try {
    await db
      .insert(screenshotMap)
      .values({
        localId,
        hostId,
        windowTitle: metadata?.windowTitle,
        processPath: metadata?.processPath,
        processName: metadata?.processName,
      })
      .onConflictDoUpdate({
        target: screenshotMap.localId,
        set: {
          hostId,
          windowTitle: metadata?.windowTitle,
          processPath: metadata?.processPath,
          processName: metadata?.processName,
        },
      })
  } catch (error) {
    console.error("Failed to map screenshot", error)
    throw error
  }
}

export const getHostId = async (localId: string) => {
  try {
    const result = await db
      .select()
      .from(screenshotMap)
      .where(eq(screenshotMap.localId, localId))
      .limit(1)
    if (!result[0]) return null
    return {
      hostId: result[0].hostId,
      windowTitle: result[0].windowTitle,
      processPath: result[0].processPath,
      processName: result[0].processName,
    }
  } catch (error) {
    console.error("Failed to get host ID", error)
    return null
  }
}


export const getLocalIds = async (hostId: string) => {
  try {
    const result = await db.select().from(screenshotMap).where(eq(screenshotMap.hostId, hostId))
    return result.map((r) => r.localId)
  } catch (error) {
    console.error("Failed to get local IDs", error)
    return []
  }
}

export const getRecentGameTitles = async (limit: number = 20) => {
  try {
    // Determine recent games by rowid (insertion order)
    // We want unique window titles, ordered by the most recent occurrence
    const result = await db
      .selectDistinct({ windowTitle: screenshotMap.windowTitle })
      .from(screenshotMap)
      .orderBy(desc(sql`rowid`)) // Assuming rowid is available or we use another heuristic
      .limit(limit * 2) // Fetch more to deduplicate in JS if needed, or rely on distinct

    // Getting distinct AND ordering by recent is tricky in standard SQL without subqueries or specific dialect support for "GROUP BY title ORDER BY MAX(rowid)".
    // SQLite: `SELECT window_title, MAX(rowid) as max_id FROM screenshot_map GROUP BY window_title ORDER BY max_id DESC` matches the intent.
    // Drizzle simplified approach:
    /*
        const result = await db.all(sql`
            SELECT window_title, MAX(rowid) as max_id 
            FROM screenshot_map 
            WHERE window_title != ''
            GROUP BY window_title 
            ORDER BY max_id DESC 
            LIMIT ${limit}
        `)
    */
   // Let's use raw SQL for this specific query as it's more efficient for "Recent Unique"
   const rawQuery = sql`
        SELECT ${screenshotMap.windowTitle}
        FROM ${screenshotMap}
        WHERE ${screenshotMap.windowTitle} != ''
        GROUP BY ${screenshotMap.windowTitle}
        ORDER BY MAX(rowid) DESC
        LIMIT ${limit}
   `
   const rawResult = await db.all(rawQuery)
   return rawResult.map((r: any) => r.window_title as string)

  } catch (error) {
    console.error("Failed to get recent game titles", error)
    return []
  }
}

export const getScreenshotsByTitle = async (
    windowTitle: string,
    limit: number = 50,
    offset: number = 0
) => {
    try {
        const result = await db
            .select({ localId: screenshotMap.localId })
            .from(screenshotMap)
            .where(eq(screenshotMap.windowTitle, windowTitle))
            .orderBy(desc(sql`rowid`)) // Most recent first
            .limit(limit)
            .offset(offset)
        
        return result.map(r => r.localId)
    } catch (error) {
        console.error("Failed to get screenshots by title", error)
        return []
    }
}
