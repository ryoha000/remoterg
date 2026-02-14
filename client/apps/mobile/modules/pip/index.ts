import { requireNativeModule } from "expo-modules-core"
import type { EventSubscription } from "expo-modules-core"

/** PiP モード変更イベントのペイロード */
export interface PipModeChangeEvent {
  isInPipMode: boolean
}

/** PiP ネイティブモジュールの型定義 */
interface PipNativeModule {
  enterPip(): void
  setAutoEnterEnabled(enabled: boolean): void
  isInPipMode(): boolean
  addListener(eventName: "onPipModeChanged", listener: (event: PipModeChangeEvent) => void): EventSubscription
  removeListener(eventName: "onPipModeChanged", listener: (event: PipModeChangeEvent) => void): void
  removeAllListeners(eventName: "onPipModeChanged"): void
}

// PiP ネイティブモジュールの取得
const PipModule = requireNativeModule<PipNativeModule>("Pip")

/**
 * 手動で PiP モードに入る
 */
export function enterPip(): void {
  PipModule.enterPip()
}

/**
 * ユーザーがアプリを離れた時の自動 PiP 切り替えを制御（Android 12+）
 */
export function setAutoEnterEnabled(enabled: boolean): void {
  PipModule.setAutoEnterEnabled(enabled)
}

/**
 * 現在 PiP モードかどうかを返す
 */
export function isInPipMode(): boolean {
  return PipModule.isInPipMode()
}

/**
 * PiP モード変更イベントのリスナーを追加
 */
export function addPipModeListener(listener: (event: PipModeChangeEvent) => void): EventSubscription {
  return PipModule.addListener("onPipModeChanged", listener)
}
