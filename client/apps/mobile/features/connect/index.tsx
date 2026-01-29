import { useRouter } from "expo-router"
import { useEffect } from "react"
import { View } from "react-native"

import { useViewerContext } from "@/features/viewer/context/ViewerContext"

import { ConnectForm } from "./components/ConnectForm"

export function ConnectScreen() {
  const router = useRouter()
  const { sessionId, setSessionId, status, connect, isConnected } = useViewerContext()

  useEffect(() => {
    if (isConnected) {
      router.push("/viewer")
    }
  }, [isConnected, router])

  return (
    <View className="flex-1 bg-background">
      <ConnectForm
        sessionId={sessionId}
        setSessionId={setSessionId}
        status={status}
        connect={connect}
        onOpenGallery={() => router.push("/gallery")}
      />
    </View>
  )
}
