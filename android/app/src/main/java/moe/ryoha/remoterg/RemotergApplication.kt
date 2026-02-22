package moe.ryoha.remoterg

import android.app.Application
import coil.ImageLoader
import coil.ImageLoaderFactory
import dagger.hilt.android.HiltAndroidApp
import javax.inject.Inject

@HiltAndroidApp
class RemotergApplication : Application(), ImageLoaderFactory {

    @Inject
    lateinit var imageLoader: ImageLoader

    override fun newImageLoader(): ImageLoader {
        return imageLoader
    }

    override fun onCreate() {
        super.onCreate()
        // Here we can initialize other global dependencies like WebRTC root Factory or logging
    }
}
