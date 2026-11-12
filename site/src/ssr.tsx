import { renderToString } from 'react-dom/server';
import { App } from './home/App';
import { NotFoundPage } from './notfound/NotFoundPage';
import { SourcesPage } from './sources/SourcesPage';

const pages = {
  home: App,
  sources: SourcesPage,
  missing: NotFoundPage,
};

export function render(page: keyof typeof pages) {
  const Page = pages[page];
  return renderToString(<Page />);
}
