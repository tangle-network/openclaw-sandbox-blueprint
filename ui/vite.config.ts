import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import UnoCSS from 'unocss/vite';
import path from 'node:path';

export default defineConfig({
  plugins: [UnoCSS(), react()],
  define: {
    global: 'globalThis',
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
    },
  },
});
