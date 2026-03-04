import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import UnoCSS from 'unocss/vite';
import path from 'node:path';

const buildMarker = process.env.UI_BUILD_MARKER ?? process.env.GITHUB_SHA?.slice(0, 12) ?? 'dev-local';

export default defineConfig({
  plugins: [UnoCSS(), react()],
  define: {
    global: 'globalThis',
    UI_BUILD_MARKER: JSON.stringify(buildMarker),
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
    rollupOptions: {
      output: {
        entryFileNames: 'app.js',
        chunkFileNames: 'assets/[name]-[hash].js',
        assetFileNames: (info) => {
          if (info.names?.some((name) => name.endsWith('.css'))) {
            return 'styles.css';
          }
          return 'assets/[name]-[hash][extname]';
        },
      },
    },
  },
  resolve: {
    dedupe: [
      'react',
      'react-dom',
      '@tanstack/react-query',
      'wagmi',
      'viem',
      '@nanostores/react',
      'nanostores',
      'framer-motion',
      'clsx',
      'tailwind-merge',
      '@tangle-network/agent-ui',
      '@tangle-network/blueprint-ui',
    ],
    alias: {
      '~': path.resolve(__dirname, 'src'),
      '@tanstack/react-query': path.resolve(__dirname, 'node_modules/@tanstack/react-query'),
    },
  },
});
