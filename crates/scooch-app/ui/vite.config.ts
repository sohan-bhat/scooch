import { resolve } from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const root = import.meta.dirname;
const site = resolve(root, '../../../site/src');

export default defineConfig(({ mode }) => ({
  base: './',
  plugins: [react(), tailwindcss()],
  publicDir: mode === 'mock' ? 'mock' : false,
  define: { __MOCK__: JSON.stringify(mode === 'mock') },
  resolve: {
    alias: { '@': site, '~': resolve(root, 'src') },
    dedupe: ['react', 'react-dom', 'radix-ui', 'class-variance-authority', 'clsx', 'tailwind-merge', '@phosphor-icons/react'],
  },
  server: { port: 5181, strictPort: true, fs: { allow: [root, site] } },
  preview: { port: 5182, strictPort: true },
  build: { target: 'es2022', emptyOutDir: true },
}));
