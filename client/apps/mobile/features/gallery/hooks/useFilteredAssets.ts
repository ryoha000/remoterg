import * as MediaLibrary from "expo-media-library"
import { useMemo } from "react"

import { SearchFilters } from "../components/SearchPanel"

export function useFilteredAssets(
  assets: MediaLibrary.Asset[],
  filters: SearchFilters,
  favoriteIds: Set<string>,
): MediaLibrary.Asset[] {
  return useMemo(() => {
    let result = assets

    // Filter by favorites
    if (filters.isFavorite) {
      result = result.filter((asset) => favoriteIds.has(asset.id))
    }

    // Filter by text (filename)
    if (filters.text) {
      const query = filters.text.toLowerCase()
      result = result.filter((asset) => asset.filename.toLowerCase().includes(query))
    }

    // Filter by date range (since)
    if (filters.since) {
      const sinceTime = filters.since.getTime()
      result = result.filter((asset) => {
        const assetTime =
          asset.creationTime > 0 ? asset.creationTime * 1000 : asset.modificationTime
        return assetTime >= sinceTime
      })
    }

    // Filter by date range (until)
    if (filters.until) {
      const untilTime = filters.until.getTime()
      result = result.filter((asset) => {
        const assetTime =
          asset.creationTime > 0 ? asset.creationTime * 1000 : asset.modificationTime
        return assetTime <= untilTime
      })
    }

    return result
  }, [assets, filters, favoriteIds])
}
