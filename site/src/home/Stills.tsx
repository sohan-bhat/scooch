import content from '@/content.json';
import { asset } from '@/lib/base';
import { cn } from '@/lib/utils';
import { BeatCopy, beatFrame, place } from './Beats';

export function Stills() {
  return (
    <section className="stills" aria-label="Scooch in seven steps">
      <ol>
        {content.beats.map((beat, index) => (
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
              <figcaption className="page-gutter absolute inset-0 tall:relative tall:inset-auto tall:pt-6 tall:pb-16">
                <div className="relative h-full w-full">
                  <div className={cn(beatFrame, place[beat.id], 'tall:relative tall:bottom-auto')}>
                    <BeatCopy beat={beat} index={index} live={false} />
                  </div>
                </div>
              </figcaption>
            </figure>
          </li>
        ))}
      </ol>
    </section>
  );
}
