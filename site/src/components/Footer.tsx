import { GithubLogo } from '@phosphor-icons/react/dist/ssr';
import content from '@/content.json';
import { asset } from '@/lib/base';

export function Footer() {
  return (
    <footer className="page-gutter border-t border-paper/10 py-12">
      <div className="flex flex-wrap items-center justify-between gap-x-10 gap-y-6">
        <a href={asset('')} className="wordmark rounded-sm text-[2rem] text-paper">
          {content.name}
        </a>
        <ul className="flex flex-wrap items-center gap-x-8 gap-y-3 text-[0.9375rem] font-medium">
          <li>
            <a href={content.github} className="link-quiet inline-flex items-center gap-2">
              <GithubLogo weight="bold" aria-hidden className="size-[1.15em]" />
              {content.footer.github}
            </a>
          </li>
          <li>
            <a href={asset('sources/')} className="link-quiet">
              {content.footer.sources}
            </a>
          </li>
          <li className="text-plastic">{content.footer.license}</li>
        </ul>
      </div>
    </footer>
  );
}
