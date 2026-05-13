// vite.config.ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'
import { sandboxRuntimePlugin } from './vite.sandbox-runtime-plugin'

export default defineConfig({
  plugins: [react(), sandboxRuntimePlugin()],

  // Expose the project-local community-plugins path so the loader can
  // scan it in dev mode without hard-coding or copying to ~/.nexus-shell/plugins/.
  define: {
    __DEV_COMMUNITY_PLUGINS_DIR__: JSON.stringify(
      path.join(__dirname, 'src', 'plugins', 'community')
    ),
  },

  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },

  // Tauri dev server — must be on a fixed port
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Tell Vite to ignore watching src-tauri
      ignored: ['**/src-tauri/**'],
    },
  },

  // Prevent Vite from obscuring Rust errors
  clearScreen: false,

  envPrefix: ['VITE_', 'TAURI_'],

  build: {
    // SH-016: Branch build target per platform so Vite emits only the
    // transforms actually needed on each OS, rather than over-targeting.
    //   windows → WebView2 (Chromium 105+)    → chrome105
    //   macos   → WebKit ≥ macOS Ventura       → safari15
    //   linux   → WebKit2GTK 2.40+ (ES2022)    → es2022
    //   default → browser preview / CI         → es2022
    target: process.env.TAURI_PLATFORM === 'windows'
      ? 'chrome105'
      : process.env.TAURI_PLATFORM === 'macos'
        ? 'safari15'
        : 'es2022',
    minify: process.env.TAURI_DEBUG ? false : 'esbuild',
    sourcemap: !!process.env.TAURI_DEBUG,
    // BL-111: pin the modulepreload polyfill OFF. The helper itself
    // is now isolated into `vite-preload-helper` via manualChunks so
    // the chunk-routing fix below stands on its own; turning the
    // polyfill off as well removes the eager `<link rel="modulepreload">`
    // tags that Vite would otherwise emit for whatever the entry
    // static-imports. Native modulepreload still works at every
    // dynamic-import call site (the helper checks
    // `link.relList.supports("modulepreload")` at runtime).
    modulePreload: { polyfill: false },
    rollupOptions: {
      output: {
        // SH-009: group heavy vendor libraries into named chunks so the
        // browser can cache them independently of plugin code changes.
        // Dynamic plugin imports (catalog.ts load() factories) each get
        // their own auto-generated chunk from Rollup's code-splitting.
        manualChunks(id) {
          // BL-111: route Vite's runtime preload helper into its own
          // tiny chunk. Without this, Rollup parks the helper inside
          // whichever named manualChunks bucket comes first by build
          // order — historically `vendor-mermaid` (~2.7 MB) — and the
          // entry chunk's single static import of the helper symbol
          // pulls the entire host chunk into the eager static-import
          // graph. Hosting it separately keeps every named manual
          // chunk genuinely lazy.
          if (id.includes('vite/preload-helper')) return 'vite-preload-helper'
          if (id.includes('node_modules/@codemirror')) return 'vendor-codemirror'
          if (id.includes('node_modules/@xterm')) return 'vendor-xterm'
          if (id.includes('node_modules/mermaid') || id.includes('node_modules/d3') || id.includes('node_modules/dagre')) return 'vendor-mermaid'
          if (id.includes('node_modules/react-dom')) return 'vendor-react'
          if (id.includes('node_modules/react/') || id.includes('node_modules/react-is')) return 'vendor-react'
        },
      },
    },
  },
})
