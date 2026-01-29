import { PortalHost } from "@rn-primitives/portal"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { Stack } from "expo-router"
import { Text, View } from "react-native"
import { GestureHandlerRootView } from "react-native-gesture-handler"

import { useMigration } from "../db/client"
import "../global.css"
import { ViewerProvider } from "@/features/viewer/context/ViewerContext"

const queryClient = new QueryClient()

export default function RootLayout() {
  const { success, error } = useMigration()

  if (error) {
    return (
      <View style={{ flex: 1, justifyContent: "center", alignItems: "center" }}>
        <Text>Migration error: {error.message}</Text>
      </View>
    )
  }

  if (!success) {
    return (
      <View style={{ flex: 1, justifyContent: "center", alignItems: "center" }}>
        <Text>Migrating database...</Text>
      </View>
    )
  }

  return (
    <QueryClientProvider client={queryClient}>
      <ViewerProvider>
        <GestureHandlerRootView style={{ flex: 1 }}>
          <Stack screenOptions={{ headerShown: false }} />
          <PortalHost />
        </GestureHandlerRootView>
      </ViewerProvider>
    </QueryClientProvider>
  )
}
