import { GithubLogo } from '@phosphor-icons/react/dist/ssr';
import { Button } from '@/components/ui/button';
import content from '@/content.json';

type Refs = {
  header: React.Ref<HTMLElement>;
  progress: React.Ref<HTMLSpanElement>;
  meter: React.Ref<HTMLSpanElement>;
  action: React.Ref<HTMLAnchorElement>;
  skip: React.Ref<HTMLAnchorElement>;
};

export function Nav({ header, progress, meter, action, skip }: Refs) {
  return (
    <header
      ref={header}
      className="page-gutter pointer-events-none fixed inset-x-0 top-0 z-30 flex h-18 items-center justify-between gap-4 border-b border-transparent transition-[background-color,border-color] duration-200 ease-(--ease-out) before:absolute before:inset-x-0 before:top-0 before:-z-10 before:h-28 before:bg-gradient-to-b before:from-room/85 before:via-room/45 before:to-transparent before:transition-opacity before:duration-200 data-solid:pointer-events-auto data-solid:border-paper/10 data-solid:bg-room data-solid:before:opacity-0 tall:h-16"
    >
      <div className="pointer-events-auto flex flex-col gap-1.5">
        <a href="#top" className="wordmark rounded-sm py-1 text-[1.65rem] text-paper transition-colors duration-(--duration-hover) ease-(--ease-out) hover-fine:text-plastic active:text-pad tall:text-[1.4rem]" aria-label={content.nav.home}>
          {content.name}
        </a>
        <span
          ref={meter}
          className="story-only relative block h-[2px] w-full overflow-hidden rounded-pill bg-paper/12 transition-opacity duration-700 ease-(--ease-out)"
          role="progressbar"
          aria-label={content.loading}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={0}
        >
          <span ref={progress} className="absolute inset-0 origin-left scale-x-0 bg-lamp transition-transform duration-300 ease-(--ease-out)" />
        </span>
      </div>
      <nav className="pointer-events-auto flex items-center gap-6 tall:gap-4" aria-label="Site">
        <a ref={skip} href="#install" className="story-only link-quiet text-[0.9375rem] font-medium">
          {content.nav.skip}
        </a>
        <a href={content.github} className="link-quiet inline-flex items-center gap-2 text-[0.9375rem] font-medium">
          <GithubLogo weight="bold" aria-hidden className="size-[1.15em]" />
          <span className="tall:sr-only">{content.nav.github}</span>
        </a>
        <Button asChild size="sm" className="story-only invisible opacity-0 tall:hidden">
          <a ref={action} href="#install">
            {content.nav.install}
          </a>
        </Button>
      </nav>
    </header>
  );
}
