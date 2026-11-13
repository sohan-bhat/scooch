import { StrictMode } from 'react';
import { createRoot, hydrateRoot } from 'react-dom/client';
import '@/styles/global.css';
import { loadManifest } from '@/story/demo';
import { App } from './App';

performance.mark('scooch:js');

const html = document.documentElement;
const story = !html.classList.contains('stills-mode') && !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
if (story) {
  void loadManifest();
  void import('@/scene/Stage');
}

const root = document.getElementById('root')!;
const tree = (
  <StrictMode>
    <App />
  </StrictMode>
);
if (root.hasChildNodes()) hydrateRoot(root, tree);
else createRoot(root).render(tree);
