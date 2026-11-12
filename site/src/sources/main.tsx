import { StrictMode } from 'react';
import { createRoot, hydrateRoot } from 'react-dom/client';
import '@/styles/global.css';
import { SourcesPage } from './SourcesPage';

const root = document.getElementById('root')!;
const tree = (
  <StrictMode>
    <SourcesPage />
  </StrictMode>
);
if (root.hasChildNodes()) hydrateRoot(root, tree);
else createRoot(root).render(tree);
