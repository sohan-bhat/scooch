import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { SplitText } from 'gsap/SplitText';
import type { DemoManifest } from './demo';
import { story } from './state';

gsap.registerPlugin(ScrollTrigger, SplitText);

export const EASE = { camera: 'power2.inOut', in: 'power3.out', out: 'power2.in' } as const;
export const DURATION = { camera: 8.5, textIn: 2.6, textOut: 2, staggerIn: 0.32, staggerOut: 0.14 } as const;

export const beats = [
  { id: 'hero', start: 0, end: 12, enter: -1, leave: 6.5, rest: 2 },
  { id: 'problem', start: 12, end: 27, enter: 15.6, leave: 23.4, rest: 21 },
  { id: 'sees', start: 27, end: 44, enter: 31, leave: 41.4, rest: 38 },
  { id: 'fix', start: 44, end: 58, enter: 45.2, leave: 55.6, rest: 54 },
  { id: 'follow', start: 58, end: 72, enter: 59, leave: 69.8, rest: 64.2 },
  { id: 'rest', start: 72, end: 88, enter: 78, leave: 85.6, rest: 83 },
  { id: 'end', start: 88, end: 100, enter: 93.2, leave: -1, rest: 99.6 },
] as const;

export type Elements = {
  beats: HTMLElement[];
  navAction: HTMLElement | null;
  skip: HTMLElement | null;
};

type Built = { timeline: gsap.core.Timeline; splits: SplitText[] };

function lines(element: HTMLElement, splits: SplitText[]) {
  const targets = element.querySelectorAll<HTMLElement>('[data-split]');
  const found: HTMLElement[] = [];
  targets.forEach((target) => {
    const split = SplitText.create(target, { type: 'lines', mask: 'lines', linesClass: 'line', aria: 'auto' });
    splits.push(split);
    found.push(...(split.lines as HTMLElement[]));
  });
  return found;
}

function to(timeline: gsap.core.Timeline, target: object, from: gsap.TweenVars, end: gsap.TweenVars, at: number, duration: number, ease: string) {
  timeline.fromTo(target, from, { ...end, duration, ease, immediateRender: false }, at);
}

export function buildTimeline(elements: Elements, manifest: DemoManifest): Built {
  const splits: SplitText[] = [];
  const timeline = gsap.timeline({ paused: true, defaults: { ease: 'none' } });
  timeline.set({}, {}, 100);

  const camera = [
    [8, 0, 1],
    [23.6, 1, 2],
    [72.2, 2, 3],
    [86.4, 3, 4],
  ] as const;
  for (const [at, from, end] of camera) to(timeline, story, { camera: from }, { camera: end }, at, DURATION.camera, EASE.camera);

  to(timeline, story, { push: 0 }, { push: 1 }, 44, 14, 'sine.inOut');
  to(timeline, story, { push: 1 }, { push: 0 }, 72.2, DURATION.camera, EASE.camera);

  to(timeline, story, { lamp: 1 }, { lamp: 0.7 }, 12, 6, EASE.camera);
  to(timeline, story, { lamp: 0.7 }, { lamp: 0 }, 73.5, 5.5, EASE.camera);
  to(timeline, story, { lamp: 0 }, { lamp: 0.55 }, 89, 6, EASE.camera);

  to(timeline, story, { dof: 0 }, { dof: 1 }, 12.5, 6, EASE.camera);
  to(timeline, story, { focus: 0 }, { focus: 1 }, 13, 6, EASE.camera);
  to(timeline, story, { dof: 1 }, { dof: 0 }, 72.2, 4, EASE.camera);

  to(timeline, story, { sweep: 0 }, { sweep: 1 }, 31.4, 5.2, EASE.camera);
  to(timeline, story, { sweep: 1 }, { sweep: 0 }, 39.6, 4, EASE.camera);

  to(timeline, story, { mixAB: 0 }, { mixAB: 1 }, 47.4, 4.6, EASE.camera);

  const icon = manifest.frames.before.icons[manifest.drag.icon];
  const grabX = manifest.drag.from[0] + icon[2] * 0.42;
  const grabY = manifest.drag.from[1] + icon[3] * 0.34;
  const restX = 0.86;
  const restY = 0.82;
  to(timeline, story, { cursorX: restX, cursorY: restY }, { cursorX: restX, cursorY: restY }, 58, 0.1, 'none');
  to(timeline, story, { cursorAlpha: 0 }, { cursorAlpha: 1 }, 58.4, 1.2, EASE.in);
  to(timeline, story, { cursorX: restX, cursorY: restY }, { cursorX: grabX, cursorY: grabY }, 59, 2.6, EASE.camera);
  to(timeline, story, { grab: 0 }, { grab: 1 }, 61.8, 0.5, EASE.in);
  to(timeline, story, { drag: 0 }, { drag: 1 }, 62.4, 3.6, EASE.camera);
  to(timeline, story, { grab: 1 }, { grab: 0 }, 66.1, 0.4, EASE.out);
  to(timeline, story, { cursorX: grabX, cursorY: grabY }, { cursorX: grabX + 0.05, cursorY: grabY + 0.07 }, 66.6, 1.4, EASE.in);
  to(timeline, story, { cursorAlpha: 1 }, { cursorAlpha: 0 }, 67.6, 1.2, EASE.out);
  to(timeline, story, { mixBC: 0 }, { mixBC: 1 }, 66.8, 3, EASE.camera);

  elements.beats.forEach((element, index) => {
    const beat = beats[index];
    const found = lines(element, splits);
    const action = element.querySelector<HTMLElement>('[data-action]');
    if (beat.enter >= 0) {
      gsap.set(found, { yPercent: 115 });
      to(timeline, found, { yPercent: 115 }, { yPercent: 0, stagger: DURATION.staggerIn }, beat.enter, DURATION.textIn, EASE.in);
      if (action) {
        gsap.set(action, { autoAlpha: 0, y: 18 });
        to(timeline, action, { autoAlpha: 0, y: 18 }, { autoAlpha: 1, y: 0 }, beat.enter + 1.2, DURATION.textIn, EASE.in);
      }
    }
    if (beat.leave >= 0) {
      to(timeline, found, { yPercent: 0 }, { yPercent: -115, stagger: DURATION.staggerOut }, beat.leave, DURATION.textOut, EASE.out);
      if (action) to(timeline, action, { autoAlpha: 1, y: 0 }, { autoAlpha: 0, y: -12 }, beat.leave, DURATION.textOut * 0.8, EASE.out);
    }
  });

  if (elements.navAction) {
    gsap.set(elements.navAction, { autoAlpha: 0, y: -8 });
    to(timeline, elements.navAction, { autoAlpha: 0, y: -8 }, { autoAlpha: 1, y: 0 }, 9, 2, EASE.in);
    to(timeline, elements.navAction, { autoAlpha: 1, y: 0 }, { autoAlpha: 0, y: -8 }, 90.5, 2, EASE.out);
  }
  if (elements.skip) to(timeline, elements.skip, { autoAlpha: 1 }, { autoAlpha: 0 }, 95, 3, EASE.out);

  return { timeline, splits };
}

export function heroLines(element: HTMLElement) {
  return element.querySelectorAll<HTMLElement>('.line');
}
