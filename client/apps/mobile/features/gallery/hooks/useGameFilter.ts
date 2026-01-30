import { useQuery } from "@tanstack/react-query"

import { getRecentGameTitles, getScreenshotsByTitle } from "@/db/services/screenshot-service"

export const useRecentGames = (limit: number = 20) => {
  return useQuery({
    queryKey: ["recentGames", limit],
    queryFn: () => getRecentGameTitles(limit),
  })
}

export const useGameScreenshots = (gameTitle: string | null) => {
  return useQuery({
    queryKey: ["gameScreenshots", gameTitle],
    queryFn: () => (gameTitle ? getScreenshotsByTitle(gameTitle) : Promise.resolve([])),
    enabled: !!gameTitle,
  })
}
