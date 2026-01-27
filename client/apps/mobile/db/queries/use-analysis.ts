import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import type { AnalysisResult } from "@/features/viewer/types"

import { getAnalysis, saveAnalysis } from "../services/analysis-service"

export const useAnalysis = (localId: string) => {
  return useQuery({
    queryKey: ["analysis", localId],
    queryFn: () => getAnalysis(localId),
    enabled: !!localId,
  })
}

export const useSaveAnalysis = () => {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ localId, analysis }: { localId: string; analysis: AnalysisResult }) => {
      await saveAnalysis(localId, analysis)
    },
    onSuccess: (_, { localId }) => {
      queryClient.invalidateQueries({ queryKey: ["analysis", localId] })
    },
  })
}
