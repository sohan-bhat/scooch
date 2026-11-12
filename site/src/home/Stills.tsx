import { Button } from '@/components/ui/button';
import content from '@/content.json';
import { asset } from '@/lib/base';
import { cn } from '@/lib/utils';

export function Stills() {
  return (
    <section className="stills" aria-label="Scooch in seven steps">
      <ol className="flex flex-col">
        {content.beats.map((beat, index) => {
          const Heading = index === 0 ? 'h1' : 'h2';
          const flip = beat.id === 'rest' || beat.id === 'end';
          return (
            <li key={beat.id} className="relative">
              <figure className="relative grid min-h-svh items-end tall:min-h-0 tall:grid-rows-[auto_auto] tall:items-start">
                <picture className="col-start-1 row-start-1 block h-full w-full tall:aspect-[390/470]">
                  <source media="(max-width: 767px), (orientation: portrait)" srcSet={asset(`stills/phone-${index + 1}.webp`)} width={780} height={940} />
                  <img
                    src={asset(`stills/desktop-${index + 1}.webp`)}
                    alt={content.stills.alt[index]}
                    width={1600}
                    height={1000}
                    loading="lazy"
                    decoding="async"
                    className="h-full w-full object-cover"
                  />
                </picture>
                <figcaption
                  className={cn(
                    'page-gutter col-start-1 row-start-1 flex flex-col items-start gap-5 self-center py-16 wide:w-[min(48vw,40rem)] tall:row-start-2 tall:pt-6 tall:pb-14',
                    flip && 'wide:justify-self-end',
                  )}
                >
                  <Heading className={cn('text-paper', index === 0 ? 'text-(length:--text-hero)' : 'text-(length:--text-beat)')}>{beat.title}</Heading>
                  {'support' in beat && beat.support ? <p className="max-w-[30ch] text-(length:--text-support) leading-snug text-plastic">{beat.support}</p> : null}
                  {'action' in beat && beat.action ? (
                    <Button asChild className="mt-2">
                      <a href="#install">{beat.action}</a>
                    </Button>
                  ) : null}
                </figcaption>
              </figure>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
