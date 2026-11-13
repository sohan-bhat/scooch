import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import Lenis from 'lenis';
import { useEffect } from 'react';
import { loadManifest, type DemoManifest } from '@/story/demo';
import { runtime, story } from '@/story/state';
import { beats as beatList, buildTimeline } from '@/story/timeline';

type Refs = {
  beats: React.RefObject<HTMLDivElement[]>;
  section: React.RefObject<HTMLElement | null>;
  canvas: React.RefObject<HTMLDivElement | null>;
  action: React.RefObject<HTMLAnchorElement | null>;
  skip: React.RefObject<HTMLAnchorElement | null>;
  still: React.RefObject<HTMLPictureElement | null>;
};

const initial = { ...story };

export function hasWebGL() {
  try {
    const canvas = document.createElement('canvas');
    return Boolean(canvas.getContext('webgl2') ?? canvas.getContext('webgl'));
  } catch {
    return false;
  }
}

export function storyMode() {
  const params = new URLSearchParams(window.location.search);
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const still = params.has('still') ? Number(params.get('still')) : -1;
  if (params.has('stills') || document.documentElement.classList.contains('stills-mode') || !hasWebGL()) return { live: false, still };
  if (reduced && still < 0) return { live: false, still };
  return { live: true, still };
}

export function useReveal() {
  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          entry.target.setAttribute('data-shown', '');
          observer.unobserve(entry.target);
        }
      },
      { rootMargin: '0px 0px -12% 0px' },
    );
    document.querySelectorAll('[data-reveal]').forEach((element) => observer.observe(element));
    return () => observer.disconnect();
  }, []);
}

export function useSolidNav(header: React.RefObject<HTMLElement | null>, end: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    const bar = header.current;
    const marker = end.current;
    if (!bar || !marker) return;
    const story = document.getElementById('story');
    if (!story || story.offsetHeight === 0) {
      bar.setAttribute('data-solid', '');
      return;
    }
    const observer = new IntersectionObserver(([entry]) => {
      const unpinned = entry.isIntersecting || entry.boundingClientRect.top < 0;
      if (unpinned) bar.setAttribute('data-solid', '');
      else bar.removeAttribute('data-solid');
    });
    observer.observe(marker);
    return () => observer.disconnect();
  }, [header, end]);
}

export function useStory(refs: Refs, live: boolean, still: number) {
  useEffect(() => {
    if (!live) return;
    const html = document.documentElement;
    html.classList.add('story-mode');
    runtime.still = still;
    if (still >= 0) {
      html.classList.add('still-shot');
      const query = new URLSearchParams(window.location.search);
      if (query.has('share')) html.classList.add('share-shot');
      if (query.has('hero')) html.classList.add('hero-shot');
      runtime.idle = false;
    }

    let disposed = false;
    let manifest: DemoManifest | null = null;
    let built: ReturnType<typeof buildTimeline> | null = null;
    let trigger: ScrollTrigger | null = null;
    let lenis: Lenis | null = null;
    let width = window.innerWidth;
    let resizeTimer = 0;
    const tick = (time: number) => lenis?.raf(time * 1000);

    const build = (at: number) => {
      if (!manifest || disposed) return;
      built = buildTimeline(
        { beats: refs.beats.current, navAction: refs.action.current, skip: refs.skip.current },
        manifest,
      );
      refs.section.current?.querySelector('.beats')?.removeAttribute('data-pending');
      if (still >= 0) {
        built.timeline.progress(beatList[Math.min(still, beatList.length - 1)].rest / 100);
        runtime.posed = true;
        return;
      }
      trigger = ScrollTrigger.create({
        trigger: refs.section.current,
        start: 'top top',
        end: 'bottom bottom',
        scrub: 0.5,
        animation: built.timeline,
        onUpdate: (self) => {
          story.progress = self.progress;
        },
      });
      const progress = at >= 0 ? at : trigger.progress;
      built.timeline.progress(progress);
      trigger.getTween()?.progress(1);
      story.progress = progress;
    };

    const teardown = () => {
      const at = trigger ? trigger.progress : -1;
      trigger?.kill();
      trigger = null;
      if (built) {
        built.timeline.progress(0).kill();
        for (const split of built.splits) split.revert();
        built = null;
      }
      Object.assign(story, initial);
      return at;
    };

    const onResize = () => {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        if (window.innerWidth === width) {
          ScrollTrigger.refresh();
          return;
        }
        width = window.innerWidth;
        build(teardown());
        ScrollTrigger.refresh();
      }, 180);
    };

    if (still < 0) {
      lenis = new Lenis({ autoRaf: false, lerp: 0.11, wheelMultiplier: 0.9, anchors: { immediate: true } });
      lenis.on('scroll', ScrollTrigger.update);
      gsap.ticker.add(tick);
      gsap.ticker.lagSmoothing(0);
      window.addEventListener('resize', onResize);
    }

    Promise.all([loadManifest(), document.fonts.ready]).then(([loaded]) => {
      if (disposed) return;
      manifest = loaded;
      build(-1);
    });

    return () => {
      disposed = true;
      window.clearTimeout(resizeTimer);
      window.removeEventListener('resize', onResize);
      teardown();
      gsap.ticker.remove(tick);
      lenis?.destroy();
    };
  }, [live, still, refs]);
}

export function introduce(refs: Refs, still: number) {
  const canvas = refs.canvas.current;
  const picture = refs.still.current;
  runtime.power = 1;
  if (canvas) canvas.style.opacity = '1';
  if (still >= 0) {
    let frames = 0;
    const wait = () => {
      if (runtime.posed) frames += 1;
      if (frames < 30) requestAnimationFrame(wait);
      else (window as Window & { __stillReady?: boolean }).__stillReady = true;
    };
    requestAnimationFrame(wait);
    return;
  }
  runtime.ready = true;
  performance.mark('scooch:ready');
  gsap.to(picture, {
    autoAlpha: 0,
    duration: 0.3,
    ease: 'power2.out',
    onComplete: () => {
      performance.mark('scooch:shown');
    },
  });
}
