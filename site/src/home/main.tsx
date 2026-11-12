import { StrictMode } from 'react';
import { createRoot, hydrateRoot } from 'react-dom/client';
import '@/styles/global.css';
import { App } from './App';

const root = document.getElementById('root')!;
const tree = (
  <StrictMode>
    <App />
  </StrictMode>
);
if (root.hasChildNodes()) hydrateRoot(root, tree);
else createRoot(root).render(tree);
