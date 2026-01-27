const { getDefaultConfig } = require("expo/metro-config")
const { withNativeWind } = require("nativewind/metro")

const config = getDefaultConfig(__dirname)

config.transformer.babelTransformerPath = require.resolve("./metro-transformer.js")
config.resolver.sourceExts.push("sql")

module.exports = withNativeWind(config, { input: "./global.css", inlineRem: 16 })
