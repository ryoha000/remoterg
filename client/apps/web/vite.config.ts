import { defineConfig } from "vite";
import { devtools } from "@tanstack/devtools-vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import viteReact from "@vitejs/plugin-react";
import viteTsConfigPaths from "vite-tsconfig-paths";
import tailwindcss from "@tailwindcss/vite";
import { viteStaticCopy } from "vite-plugin-static-copy";

const config = defineConfig({
  plugins: [
    devtools(),
    // パスエイリアスを有効にするプラグイン
    viteTsConfigPaths({
      projects: ["./tsconfig.json"],
    }),
    tailwindcss(),
    tanstackStart(),
    viteReact({
      babel: {
        plugins: ["babel-plugin-react-compiler"],
      },
    }),
    viteStaticCopy({
      targets: [
        {
          src: "instrument.server.mjs",
          dest: ".output/server",
        },
      ],
    }),
  ],
  optimizeDeps: {
    exclude: ["@remoterg/core", "@remoterg/webrtc", "@remoterg/ui"],
  },
  server: {
    host: '0.0.0.0',
    allowedHosts: true,
  },
});

export default config;
