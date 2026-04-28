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
    // Tauri supports ES2021
    target: process.env.TAURI_PLATFORM === 'windows'
      ? 'chrome105'
      : 'safari13',
    minify: process.env.TAURI_DEBUG ? false : 'esbuild',
    sourcemap: !!process.env.TAURI_DEBUG,
  },
})
