import { ArrowLeft } from '@phosphor-icons/react/dist/ssr';
import { Button } from '@/components/ui/button';
import content from '@/content.json';
import { asset } from '@/lib/base';

export function NotFoundPage() {
  const { missing } = content;
  return (
    <main className="page-gutter relative grid min-h-dvh place-items-center overflow-hidden py-24">
      <div aria-hidden className="absolute inset-0 bg-[radial-gradient(50%_45%_at_50%_42%,rgb(229_89_26/0.16),transparent_70%)]" />
      <div className="relative flex flex-col items-center gap-8 text-center">
        <a href={asset('')} className="wordmark rounded-sm text-[clamp(4rem,14vw,10rem)] text-paper" aria-label={missing.home}>
          {content.name}
        </a>
        <h1 className="sr-only">{missing.title}</h1>
        <p className="max-w-[24ch] text-(length:--text-support) text-plastic">{missing.line}</p>
        <Button asChild size="md">
          <a href={asset('')}>
            <ArrowLeft weight="bold" aria-hidden />
            {missing.home}
          </a>
        </Button>
      </div>
    </main>
  );
}
