import { defineConfig } from "vite";
import { devtools } from "@tanstack/devtools-vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import viteReact from "@vitejs/plugin-react";
import viteTsConfigPaths from "vite-tsconfig-paths";
import tailwindcss from "@tailwindcss/vite";
import { cloudflare } from "@cloudflare/vite-plugin";
import { viteStaticCopy } from "vite-plugin-static-copy";

const config = defineConfig({
  plugins: [
    devtools(),
    cloudflare({
      viteEnvironment: { name: "ssr" },
      configPath: "./wrangler.jsonc",
    }),
    // this is the plugin that enables path aliases
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
    host: true,
  },
});

export default config;
