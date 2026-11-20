import { FilmStrip, type Icon, Image, ImageBroken, PaintBucket, Slideshow } from '@phosphor-icons/react';
import { useCallback, useEffect, useState } from 'react';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { Backend } from '~/backend';
import type { Picture, Problem, SwitchName, View, WallpaperKind } from '~/backend/types';
import { copy } from '~/lib/copy';
import { Desk } from '~/parts/Desk';
import { Pictures } from '~/parts/Pictures';
import { type Act, Status, said } from '~/parts/Status';
import { Switches } from '~/parts/Switches';

const BLOCKING: Problem['kind'][] = ['unknown', 'move'];

const EMPTY: Record<WallpaperKind, Icon> = { slideshow: Slideshow, video: FilmStrip, moving: FilmStrip, color: PaintBucket, unreadable: ImageBroken, made: Image, other: Image };

export function App({ backend, first }: { backend: Backend; first: View }) {
  const [view, setView] = useState(first);
  const [pictures, setPictures] = useState<Picture[]>([]);
  const [dropping, setDropping] = useState(false);

  const refresh = useCallback(() => backend.view().then(setView), [backend]);

  useEffect(() => backend.subscribe(refresh), [backend, refresh]);
  useEffect(() => {
    backend.pictures().then(setPictures);
  }, [backend, view.current]);
  useEffect(() => {
    const onFocus = () => refresh();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refresh]);
  useEffect(
    () =>
      backend.onDrop((hovering, path) => {
        setDropping(hovering);
        if (path) backend.use(path);
      }),
    [backend]
  );
  useEffect(() => {
    document.documentElement.style.setProperty('--text-scale', String(view.textScale));
  }, [view.textScale]);
  useEffect(() => {
    document.documentElement.toggleAttribute('data-calm', view.reduceMotion);
  }, [view.reduceMotion]);
  useEffect(() => {
    backend.firstOpen().then((first) => {
      if (first) backend.turnOn();
    });
  }, [backend]);

  const pick = useCallback(async () => {
    const path = await backend.pick();
    if (path) await backend.use(path);
  }, [backend]);

  const blocked = view.problem !== null && BLOCKING.includes(view.problem.kind);
  const words = said(
    view,
    (problem) => backend.fix(problem),
    pick,
    () => backend.set('flip', true)
  );
  const on = view.phase === 'on' || view.phase === 'starting';
  const busy = view.phase === 'starting' || view.phase === 'stopping';
  const label = view.phase === 'starting' ? copy.pill.starting : view.phase === 'stopping' ? copy.pill.stopping : on ? copy.pill.off : copy.pill.on;
  const cannotTurnOn = view.problem?.kind === 'admin' && !on;
  const toggle: Act | null = blocked || cannotTurnOn ? null : { label, busy, run: () => (on ? backend.turnOff() : backend.turnOn()) };
  const pill = words.fix ?? toggle;
  const quiet = words.fix ? toggle : null;

  return (
    <TooltipProvider delayDuration={500}>
      <main className="grid h-full grid-cols-[minmax(0,1fr)] grid-rows-[auto_minmax(0,1fr)_auto_minmax(0,auto)] bg-[radial-gradient(110%_80%_at_50%_42%,var(--color-room-2),var(--color-room)_72%)]" data-system={view.system}>
        <header data-tauri-drag-region className={`flex h-16 items-center justify-between gap-6 px-6 cramped:h-14 cramped:px-4 ${view.system === 'macos' ? 'pl-24 cramped:pl-[5.5rem]' : ''}`}>
          <h1 data-tauri-drag-region className="wordmark m-0 text-[1.625rem] text-paper cramped:text-[1.375rem]">{copy.name}</h1>
          {!blocked && (
            <div className="-m-2 min-w-0 overflow-x-auto p-2 [scrollbar-width:none]">
              <Switches values={view.switches} onChange={(name: SwitchName, value) => backend.set(name, value)} />
            </div>
          )}
        </header>
        <div className="min-h-0 px-10 py-3 cramped:px-6 cramped:py-2">
          <Desk monitors={view.monitors} system={view.system} empty={view.problem?.kind === 'wallpaper' ? EMPTY[view.problem.what] : undefined} />
        </div>
        <Status line={words.line} pill={pill} quiet={quiet} />
        {!blocked && <Pictures pictures={pictures} current={view.current} onUse={(path) => backend.use(path)} onPick={pick} />}
      </main>
      {dropping && (
        <div className="fixed inset-0 z-10 grid place-items-center bg-room/90 p-4">
          <div className="grid size-full place-items-center rounded-panel border-2 border-dashed border-paper/35">
            <p className="m-0 font-(family-name:--font-display) text-[2rem] font-bold tracking-[-0.03em] text-paper">{copy.pictures.drop}</p>
          </div>
        </div>
      )}
    </TooltipProvider>
  );
}
