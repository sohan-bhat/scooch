import { Check, Copy } from '@phosphor-icons/react/dist/ssr';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import content from '@/content.json';

function CopyCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  const [open, setOpen] = useState(false);
  const timer = useRef<number>(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
    } catch {
      const area = document.createElement('textarea');
      area.value = command;
      document.body.append(area);
      area.select();
      document.execCommand('copy');
      area.remove();
    }
    setCopied(true);
    setOpen(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      setCopied(false);
      setOpen(false);
    }, 1600);
  };
  const label = copied ? content.install.copied : content.install.copy;
  return (
    <Tooltip open={open} onOpenChange={(next) => setOpen(next || copied)}>
      <TooltipTrigger asChild>
        <Button variant="quiet" size="icon" onClick={copy} aria-label={label}>
          <span className="relative grid size-5 place-items-center">
            <Copy weight="bold" className={copied ? 'scale-50 opacity-0 transition duration-200 ease-(--ease-out)' : 'transition duration-200 ease-(--ease-out)'} />
            <Check weight="bold" className={copied ? 'absolute text-pad transition duration-200 ease-(--ease-out)' : 'absolute scale-50 opacity-0 transition duration-200 ease-(--ease-out)'} />
          </span>
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  );
}

function platform() {
  const agent = `${navigator.userAgent} ${(navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ?? ''}`;
  if (/Windows/i.test(agent)) return 'windows';
  if (/Linux|X11/i.test(agent) && !/Android/i.test(agent)) return 'linux';
  return 'macos';
}

export function Install() {
  const [tab, setTab] = useState('macos');
  useEffect(() => setTab(platform()), []);
  const { install } = content;
  return (
    <section id="install" className="page-gutter relative scroll-mt-20 pt-[clamp(6rem,14vw,11rem)] pb-[clamp(5rem,10vw,8rem)]" aria-labelledby="install-title">
      <div className="grid gap-10 wide:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] wide:items-end wide:gap-[6vw]">
        <h2 id="install-title" data-reveal className="text-(length:--text-beat) text-paper">
          {install.title}
        </h2>
        <TooltipProvider>
          <Tabs value={tab} onValueChange={setTab} data-reveal style={{ '--reveal-index': 1 } as React.CSSProperties} className="min-w-0">
            <TabsList aria-label="Operating system">
              {install.tabs.map((item) => (
                <TabsTrigger key={item.id} value={item.id}>
                  {item.label}
                </TabsTrigger>
              ))}
            </TabsList>
            {install.tabs.map((item) => (
              <TabsContent key={item.id} value={item.id} className="data-[state=active]:animate-[tip-in_240ms_var(--ease-out)]">
                <div className="flex items-center gap-3 rounded-panel bg-room-2 py-2.5 pr-2.5 pl-5 shadow-[inset_0_0_0_1px_rgb(243_236_220/0.08)]">
                  <code className="min-w-0 flex-1 overflow-x-auto py-2 font-mono text-[0.875rem] leading-relaxed whitespace-nowrap text-paper [scrollbar-width:thin] tall:text-[0.8125rem] tall:break-all tall:whitespace-normal">
                    <span aria-hidden className="mr-3 text-lamp select-none">$</span>
                    {item.command}
                  </code>
                  <CopyCommand command={item.command} />
                </div>
              </TabsContent>
            ))}
            <p className="text-(length:--text-support) text-plastic">
              {install.after.replace(/scooch\.$/, '')}
              <code className="mx-0.5 rounded-md bg-room-2 px-1.5 py-0.5 font-mono text-[0.9em] text-paper">scooch</code>.
            </p>
          </Tabs>
        </TooltipProvider>
      </div>
    </section>
  );
}
