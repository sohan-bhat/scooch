import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Footer } from '@/components/Footer';
import { release } from '@/lib/load';
import { runtime } from '@/story/state';
import { Facts } from './Facts';
import { Install } from './Install';
import { Nav } from './Nav';
import { Stills } from './Stills';
import { Story } from './Story';
import { introduce, storyMode, useReveal, useSolidNav, useStory } from './useStory';

function armMusic() {
  void import('@/lib/audio').then((audio) => audio.arm());
}

function afterLoad() {
  if (document.readyState === 'complete') armMusic();
  else window.addEventListener('load', armMusic, { once: true });
}

export function App() {
  const beats = useRef<HTMLElement[]>([]);
  const section = useRef<HTMLElement>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const action = useRef<HTMLAnchorElement>(null);
  const skip = useRef<HTMLAnchorElement>(null);
  const still = useRef<HTMLPictureElement>(null);
  const header = useRef<HTMLElement>(null);
  const storyEnd = useRef<HTMLDivElement>(null);
  const refs = useMemo(() => ({ beats, section, canvas, still, action, skip }), []);
  const [mode, setMode] = useState({ live: false, still: -1 });

  useEffect(() => {
    const next = storyMode();
    if (!next.live) {
      document.documentElement.classList.add('stills-mode');
      release(false);
      if (next.still < 0) afterLoad();
    }
    setMode(next);
    const fallback = () => {
      runtime.abandoned = true;
      runtime.resume?.();
      setMode((current) => ({ ...current, live: false }));
      afterLoad();
    };
    window.addEventListener('scooch:stills', fallback);
    return () => window.removeEventListener('scooch:stills', fallback);
  }, []);

  useStory(refs, mode.live, mode.still);
  useReveal();
  useSolidNav(header, storyEnd);

  const onReady = useCallback(() => {
    introduce(refs, mode.still);
    if (mode.still < 0) armMusic();
  }, [refs, mode.still]);

  return (
    <>
      <div id="top" className="absolute top-0" />
      <Nav header={header} action={action} skip={skip} />
      <main>
        <Story live={mode.live} beats={beats} canvas={canvas} still={still} section={section} onReady={onReady} />
        <Stills />
        <div ref={storyEnd} aria-hidden className="h-px" />
        <Install />
        <Facts />
      </main>
      <Footer />
    </>
  );
}
