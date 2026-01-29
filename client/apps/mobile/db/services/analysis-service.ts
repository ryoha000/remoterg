import { eq } from "drizzle-orm"

import type { AnalysisResult } from "@/features/gallery/types"

import { db } from "../client"
import { analysisResults } from "../schema/analysis"

export const saveAnalysis = async (localId: string, analysis: AnalysisResult) => {
  try {
    await db
      .insert(analysisResults)
      .values({
        localId,
        data: JSON.stringify(analysis),
        createdAt: Date.now(),
      })
      .onConflictDoUpdate({
        target: analysisResults.localId,
        set: {
          data: JSON.stringify(analysis),
          createdAt: Date.now(),
        },
      })
  } catch (error) {
    console.error("Failed to save analysis", error)
    throw error
  }
}

export const getAnalysis = async (localId: string): Promise<AnalysisResult | null> => {
  try {
    const result = await db
      .select()
      .from(analysisResults)
      .where(eq(analysisResults.localId, localId))
      .limit(1)

    if (result[0]) {
      return JSON.parse(result[0].data) as AnalysisResult
    }
    return null
  } catch (error) {
    console.error("Failed to get analysis", error)
    return null
  }
}
