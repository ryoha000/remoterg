import React, { createContext, useContext, ReactNode } from "react"
import { useViewer } from "../hooks/useViewer"

type UseViewerResult = ReturnType<typeof useViewer>

const ViewerContext = createContext<UseViewerResult | null>(null)

export function ViewerProvider({ children }: { children: ReactNode }) {
  const viewer = useViewer()

  return <ViewerContext.Provider value={viewer}>{children}</ViewerContext.Provider>
}

export function useViewerContext() {
  const context = useContext(ViewerContext)
  if (!context) {
    throw new Error("useViewerContext must be used within a ViewerProvider")
  }
  return context
}
