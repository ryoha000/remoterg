package moe.ryoha.remoterg.di

import android.app.Application
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import moe.ryoha.remoterg.webrtc.WebRtcManager
import moe.ryoha.remoterg.webrtc.signaling.SignalingClient
import javax.inject.Singleton

/**
 * WebRTC 関連の依存性を提供する Hilt モジュール
 *
 * Application context は Hilt が自動的に提供する
 */
@Module
@InstallIn(SingletonComponent::class)
object WebRtcModule {

    @Provides
    @Singleton
    fun provideWebRtcManager(): WebRtcManager {
        return WebRtcManager()
    }

    @Provides
    @Singleton
    fun provideSignalingClient(): SignalingClient {
        return SignalingClient()
    }
}
