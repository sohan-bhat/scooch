import { StrictMode } from 'react';
import { createRoot, hydrateRoot } from 'react-dom/client';
import '@/styles/global.css';
import { NotFoundPage } from './NotFoundPage';

const root = document.getElementById('root')!;
const tree = (
  <StrictMode>
    <NotFoundPage />
  </StrictMode>
);
if (root.hasChildNodes()) hydrateRoot(root, tree);
else createRoot(root).render(tree);
