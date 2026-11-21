import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { SwitchName, Switches as Values } from '~/backend/types';
import { Label } from '~/components/ui/label';
import { Switch } from '~/components/ui/switch';
import { copy } from '~/lib/copy';

const ORDER: SwitchName[] = ['glide', 'scenery', 'flip', 'predict'];

export function Switches({ values, onChange }: { values: Values; onChange: (name: SwitchName, on: boolean) => void }) {
  const shown = ORDER.filter((name) => values[name] !== undefined);
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-[24px] gap-y-[8px] cramped:gap-x-[16px]">
      {shown.map((name) => {
        const id = `switch-${name}`;
        return (
          <Tooltip key={name}>
            <TooltipTrigger asChild>
              <div className="flex items-center gap-[10px]">
                <Label htmlFor={id} className="cramped:text-[0.875rem]">
                  {copy.switch[name].label}
                </Label>
                <Switch id={id} checked={values[name] ?? false} onCheckedChange={(on) => onChange(name, on)} />
              </div>
            </TooltipTrigger>
            <TooltipContent side="bottom">{copy.switch[name].tip}</TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
}
