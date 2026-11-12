import { GithubLogo } from '@phosphor-icons/react/dist/ssr';
import content from '@/content.json';
import { asset } from '@/lib/base';

export function Header() {
  return (
    <header className="page-gutter absolute inset-x-0 top-0 z-30 flex h-18 items-center justify-between gap-4 tall:h-16">
      <a href={asset('')} className="wordmark rounded-sm py-1 text-[1.65rem] text-paper transition-colors duration-(--duration-hover) ease-(--ease-out) hover-fine:text-plastic active:text-pad tall:text-[1.4rem]" aria-label={content.nav.home}>
        {content.name}
      </a>
      <nav aria-label="Site">
        <a href={content.github} className="link-quiet inline-flex items-center gap-2 text-[0.9375rem] font-medium">
          <GithubLogo weight="bold" aria-hidden className="size-[1.15em]" />
          <span className="tall:sr-only">{content.nav.github}</span>
        </a>
      </nav>
    </header>
  );
}
