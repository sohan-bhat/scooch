import content from '@/content.json';
import { asset } from '@/lib/base';
import { Closing, Hero } from './Beats';

function Words({ id, caption }: { id: string; caption?: string }) {
  if (id === 'hero') return <Hero live={false} />;
  if (id === 'end') return <Closing />;
  if (caption) return <p className="text-(length:--text-beat) text-paper">{caption}</p>;
  return null;
}

export function Stills() {
  return (
    <section className="stills" aria-label={content.story.label}>
      <ol>
        {content.beats.map((beat, index) => {
          const caption = 'caption' in beat ? beat.caption : undefined;
          const words = beat.id === 'hero' || beat.id === 'end' || caption;
          return (
            <li key={beat.id}>
              <figure className="relative h-svh min-h-[34rem] overflow-hidden tall:h-auto tall:min-h-0">
                <picture className="absolute inset-0 block tall:relative tall:aspect-[390/470]">
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
                {words ? (
                  <figcaption className="page-gutter absolute inset-x-0 bottom-0 flex flex-col items-start gap-5 bg-gradient-to-t from-room via-room/80 to-transparent pt-24 pb-[8svh] tall:relative tall:bg-none tall:pt-6 tall:pb-16">
                    <Words id={beat.id} caption={caption} />
                  </figcaption>
                ) : null}
              </figure>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
