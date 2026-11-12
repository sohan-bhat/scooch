import { resolve } from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const root = import.meta.dirname;

export default defineConfig({
  base: '/scooch/',
  appType: 'mpa',
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': resolve(root, 'src') } },
  server: { port: 5179, strictPort: true },
  preview: { port: 4173, strictPort: true },
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 1800,
    rollupOptions: {
      input: {
        main: resolve(root, 'index.html'),
        sources: resolve(root, 'sources/index.html'),
        missing: resolve(root, '404.html'),
      },
    },
  },
});
