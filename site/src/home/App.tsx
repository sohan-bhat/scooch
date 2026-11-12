import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Footer } from '@/components/Footer';
import { Facts } from './Facts';
import { Install } from './Install';
import { Nav } from './Nav';
import { Stills } from './Stills';
import { Story } from './Story';
import { introduce, reportProgress, storyMode, useReveal, useStory } from './useStory';

export function App() {
  const beats = useRef<HTMLDivElement[]>([]);
  const section = useRef<HTMLElement>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const action = useRef<HTMLAnchorElement>(null);
  const skip = useRef<HTMLAnchorElement>(null);
  const progress = useRef<HTMLSpanElement>(null);
  const meter = useRef<HTMLSpanElement>(null);
  const refs = useMemo(() => ({ beats, section, canvas, action, skip, progress, meter }), []);
  const [mode, setMode] = useState({ live: false, still: -1 });

  useEffect(() => {
    const next = storyMode();
    if (!next.live) document.documentElement.classList.add('stills-mode');
    setMode(next);
  }, []);

  useStory(refs, mode.live, mode.still);
  useReveal();

  const onProgress = useCallback((value: number) => reportProgress(refs, value), [refs]);
  const onReady = useCallback(() => introduce(refs, mode.still), [refs, mode.still]);

  return (
    <>
      <div id="top" className="absolute top-0" />
      <Nav progress={progress} meter={meter} action={action} skip={skip} />
      <main>
        <Story live={mode.live} beats={beats} canvas={canvas} section={section} onProgress={onProgress} onReady={onReady} />
        <Stills />
        <Install />
        <Facts />
      </main>
      <Footer />
    </>
  );
}
