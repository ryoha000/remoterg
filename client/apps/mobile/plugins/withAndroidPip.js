// @ts-check
const { withAndroidManifest } = require("expo/config-plugins")

/**
 * AndroidManifest.xml に PiP (Picture-in-Picture) 対応の属性を追加する Config Plugin
 *
 * - android:supportsPictureInPicture="true" を MainActivityに追加
 * - android:configChanges に PiP 遷移時に必要な値を追加
 */
const withAndroidPip = (config) => {
  return withAndroidManifest(config, (config) => {
    const manifest = config.modResults
    const application = manifest.manifest.application?.[0]
    if (!application) return config

    const activities = application.activity
    if (!activities) return config

    // MainActivity を検索
    const mainActivity = activities.find(
      (activity) =>
        activity.$?.["android:name"] === ".MainActivity",
    )
    if (!mainActivity) return config

    // PiP サポートを有効化
    mainActivity.$["android:supportsPictureInPicture"] = "true"

    // configChanges に PiP 遷移時のコンフィグ変更を追加
    const currentChanges = mainActivity.$["android:configChanges"] || ""
    const requiredChanges = ["smallestScreenSize", "screenLayout", "screenSize"]
    const existingChanges = currentChanges.split("|").filter(Boolean)

    for (const change of requiredChanges) {
      if (!existingChanges.includes(change)) {
        existingChanges.push(change)
      }
    }

    mainActivity.$["android:configChanges"] = existingChanges.join("|")

    return config
  })
}

module.exports = withAndroidPip
