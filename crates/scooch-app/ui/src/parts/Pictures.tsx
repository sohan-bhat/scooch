import { Plus } from '@phosphor-icons/react';
import { Button } from '@/components/ui/button';
import type { Picture } from '~/backend/types';
import { ScrollArea } from '~/components/ui/scroll-area';
import { ToggleGroup, ToggleGroupItem } from '~/components/ui/toggle-group';
import { copy } from '~/lib/copy';

const nameOf = (path: string) => path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '').replace(/[-_]+/g, ' ') ?? path;

export function Pictures({ pictures, current, onUse, onPick }: { pictures: Picture[]; current: string | null; onUse: (path: string) => void; onPick: () => void }) {
  const selected = current ?? '';
  return (
    <ScrollArea className="w-full">
      <div className="flex items-center gap-3 px-6 pt-2 pb-4 cramped:px-4 cramped:pb-3">
        <Button variant="quiet" onClick={onPick} className="h-[4.375rem] w-28 shrink-0 flex-col gap-1 px-0 text-[0.8125rem] cramped:h-15 cramped:w-24">
          <Plus weight="bold" aria-hidden />
          {copy.pictures.pick}
        </Button>
        <ToggleGroup type="single" value={selected} onValueChange={(path) => path && onUse(path)} aria-label={copy.pictures.label}>
          {pictures.map((picture) => (
            <ToggleGroupItem key={picture.path} value={picture.path} aria-label={nameOf(picture.path)} className="h-[4.375rem] w-28 cramped:h-15 cramped:w-24">
              <img src={picture.thumb} alt="" draggable={false} loading="lazy" className="size-full object-cover" />
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
    </ScrollArea>
  );
}
