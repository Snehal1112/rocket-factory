import path from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"
import { viteSingleFile } from "vite-plugin-singlefile"

// The whole point of this build is one file a reader can download and
// open from disk, so every asset -- JS, CSS and the JetBrains Mono woff2
// subsets -- is inlined. `assetsInlineLimit: Infinity` covers those, which are
// well past Vite's 4 kB default and would otherwise be emitted as siblings
// that a shared-by-email HTML file could never find.
export default defineConfig(({ command }) => ({
  // Build only. The plugin's config hook sets `base: "./"` unconditionally,
  // which is right for a file:// artifact and wrong for the dev server, where
  // Vite normalises "./" to "/" -- so the plugin is left out of dev.
  plugins: [
    react(),
    tailwindcss(),
    ...(command === "build" ? [viteSingleFile()] : []),
  ],
  build: {
    assetsInlineLimit: Number.POSITIVE_INFINITY,
    cssCodeSplit: false,
  },
  server: {
    host: "127.0.0.1",
    port: 5175,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
}))
