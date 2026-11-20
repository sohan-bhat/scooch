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
    <ScrollArea className="min-h-0 w-full">
      <div className="flex items-center gap-[12px] px-[24px] pt-[8px] pb-[16px] cramped:px-[16px] cramped:pb-[12px]">
        <Button variant="quiet" onClick={onPick} className="h-auto min-h-[70px] w-auto min-w-[112px] shrink-0 flex-col gap-[4px] px-[10px] py-[6px] text-[0.8125rem] cramped:min-h-[60px] cramped:min-w-[96px]">
          <Plus weight="bold" aria-hidden />
          {copy.pictures.pick}
        </Button>
        <ToggleGroup className="gap-[12px]" type="single" value={selected} onValueChange={(path) => path && onUse(path)} aria-label={copy.pictures.label}>
          {pictures.map((picture) => (
            <ToggleGroupItem key={picture.path} value={picture.path} aria-label={nameOf(picture.path)} className="h-[70px] w-[112px] cramped:h-[60px] cramped:w-[96px]">
              <img src={picture.thumb} alt="" draggable={false} loading="lazy" className="size-full object-cover" />
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
    </ScrollArea>
  );
}
