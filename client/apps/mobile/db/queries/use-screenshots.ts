import { useMutation, useQuery } from "@tanstack/react-query"

import { getHostId, getLocalIds, mapScreenshot } from "../services/screenshot-service"

export const useHostId = (localId: string) => {
  return useQuery({
    queryKey: ["screenshots", "hostId", localId],
    queryFn: () => getHostId(localId),
    enabled: !!localId,
  })
}

export const useLocalIds = (hostId: string) => {
  return useQuery({
    queryKey: ["screenshots", "localIds", hostId],
    queryFn: () => getLocalIds(hostId),
    enabled: !!hostId,
  })
}

export const useMapScreenshot = () => {
  return useMutation({
    mutationFn: async ({ localId, hostId }: { localId: string; hostId: string }) => {
      await mapScreenshot(localId, hostId)
    },
  })
}
