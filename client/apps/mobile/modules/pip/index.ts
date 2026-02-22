import { requireNativeModule } from "expo-modules-core"
import type { EventSubscription } from "expo-modules-core"

/** PiP モード変更イベントのペイロード */
export interface PipModeChangeEvent {
  isInPipMode: boolean
}

/** PiP ネイティブモジュールの型定義 */
interface PipNativeModule {
  enterPip(width: number, height: number, x: number, y: number, w: number, h: number): void
  setAutoEnterEnabled(enabled: boolean, width: number, height: number, x: number, y: number, w: number, h: number): void
  setPipParams(width: number, height: number, x: number, y: number, w: number, h: number): void
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
export function enterPip(width = 0, height = 0, x = 0, y = 0, w = 0, h = 0): void {
  PipModule.enterPip(width, height, x, y, w, h)
}

/**
 * ユーザーがアプリを離れた時の自動 PiP 切り替えを制御（Android 12+）
 */
export function setAutoEnterEnabled(enabled: boolean, width = 0, height = 0, x = 0, y = 0, w = 0, h = 0): void {
  PipModule.setAutoEnterEnabled(enabled, width, height, x, y, w, h)
}

/**
 * PiP パラメータ（アスペクト比、ソース矩形）を更新する
 */
export function setPipParams(width = 0, height = 0, x = 0, y = 0, w = 0, h = 0): void {
  PipModule.setPipParams(width, height, x, y, w, h)
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
