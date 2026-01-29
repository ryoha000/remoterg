import { eq } from "drizzle-orm"

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
