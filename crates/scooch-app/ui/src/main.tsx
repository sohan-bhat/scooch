import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { connect } from '~/backend';
import { App } from '~/App';
import '~/styles/app.css';

const backend = await connect();
const first = await backend.view();
document.documentElement.style.setProperty('--text-scale', String(first.textScale));

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App backend={backend} first={first} />
    </StrictMode>
  );
}
