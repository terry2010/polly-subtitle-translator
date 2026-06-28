import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // WebView2 支持现代 JS，无需降级；跳过 polyfill 加速解析
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      output: {
        // 拆分大依赖，首屏只加载必要的 chunk
        manualChunks: {
          "react-vendor": ["react", "react-dom"],
          "router": ["react-router-dom"],
          "i18n": ["i18next", "react-i18next"],
          "tauri-api": ["@tauri-apps/api", "@tauri-apps/plugin-dialog", "@tauri-apps/plugin-fs", "@tauri-apps/plugin-os", "@tauri-apps/plugin-process", "@tauri-apps/plugin-shell", "@tauri-apps/plugin-notification"],
          "ui-vendor": ["@radix-ui/react-dialog", "@radix-ui/react-dropdown-menu", "@radix-ui/react-select", "@radix-ui/react-tabs", "@radix-ui/react-tooltip", "@radix-ui/react-checkbox", "@radix-ui/react-scroll-area", "@radix-ui/react-progress", "@tanstack/react-table", "@tanstack/react-virtual", "lucide-react", "sonner"],
        },
      },
    },
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
        }
      : undefined,
    watch: {
      ignored: [
        "**/src-tauri/**",
        "**/*.srt", "**/*.srt", "**/*.ass", "**/*.ssa", "**/*.vtt",
        "**/*.mkv", "**/*.mp4", "**/*.avi", "**/*.mov",
      ],
    },
  },
}));
