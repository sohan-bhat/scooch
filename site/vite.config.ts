import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';

const root = import.meta.dirname;
const benchmarks = resolve(root, 'node_modules/detect-gpu/dist/benchmarks');

function gpuBenchmarks(): Plugin {
  return {
    name: 'gpu-benchmarks',
    apply: 'build',
    generateBundle() {
      for (const name of readdirSync(benchmarks)) this.emitFile({ type: 'asset', fileName: `benchmarks/${name}`, source: readFileSync(resolve(benchmarks, name)) });
    },
  };
}

export default defineConfig({
  base: '/scooch/',
  appType: 'mpa',
  plugins: [react(), tailwindcss(), gpuBenchmarks()],
  resolve: { alias: { '@engine': resolve(root, '../crates/scooch-web/pkg'), '@': resolve(root, 'src') } },
  worker: { format: 'es' },
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
