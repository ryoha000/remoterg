import { useRouter } from "expo-router"

import { GalleryView } from "@/features/gallery/components/GalleryView"

export default function GalleryRoute() {
  const router = useRouter()

  return (
    <GalleryView
      onBack={() => router.back()}
      // Offline mode: no session-based analysis or request function
    />
  )
}
