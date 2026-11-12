import content from '@/content.json';
import { cn } from '@/lib/utils';

const offsets = ['', 'wide:ml-[18vw]', 'wide:ml-[8vw]'];

export function Facts() {
  return (
    <section className="page-gutter pb-[clamp(6rem,12vw,10rem)]" aria-label="What to know">
      <ul className="flex flex-col border-t border-paper/10">
        {content.facts.map((fact, index) => (
          <li
            key={fact}
            data-reveal
            style={{ '--reveal-index': index } as React.CSSProperties}
            className={cn('border-b border-paper/10 py-[clamp(1.25rem,3vw,2.25rem)]', offsets[index])}
          >
            <p className="font-display text-(length:--text-fact) leading-[1.02] font-[650] tracking-[-0.025em] text-paper [font-stretch:108%]">
              {fact}
            </p>
          </li>
        ))}
      </ul>
    </section>
  );
}
