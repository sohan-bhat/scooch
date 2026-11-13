import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { SplitText } from 'gsap/SplitText';
import type { DemoManifest } from './demo';
import { story } from './state';

gsap.registerPlugin(ScrollTrigger, SplitText);

export const EASE = { camera: 'power2.inOut', in: 'power3.out', out: 'power2.in' } as const;
export const DURATION = { camera: 8.5, textIn: 2.6, textOut: 2, staggerIn: 0.32, staggerOut: 0.14 } as const;

export const TOTAL = 120;

export const beats = [
  { id: 'hero', start: 0, end: 12, enter: -1, leave: 6.5, rest: 2 },
  { id: 'problem', start: 12, end: 30, enter: 19.5, leave: 27.4, rest: 25.2 },
  { id: 'sees', start: 30, end: 50, enter: 39.5, leave: 47.4, rest: 45 },
  { id: 'fix', start: 50, end: 64, enter: 52.4, leave: 61.6, rest: 58.4 },
  { id: 'follow', start: 64, end: 90, enter: 65, leave: 87.8, rest: 85.4 },
  { id: 'rest', start: 90, end: 106, enter: 99, leave: 103.8, rest: 101.6 },
  { id: 'end', start: 106, end: TOTAL, enter: 111.4, leave: -1, rest: 119.6 },
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
  timeline.set({}, {}, TOTAL);

  to(timeline, story, { camera: 0 }, { camera: 2 }, 8, 15.5, EASE.camera);
  to(timeline, story, { camera: 2 }, { camera: 3 }, 30, 4.4, EASE.camera);
  to(timeline, story, { camera: 3 }, { camera: 4 }, 34.2, 5.6, EASE.camera);
  to(timeline, story, { camera: 4 }, { camera: 5 }, 50, 9, EASE.camera);
  to(timeline, story, { camera: 5 }, { camera: 6 }, 64, 12, EASE.camera);
  to(timeline, story, { camera: 6 }, { camera: 7 }, 90, 9.4, EASE.camera);
  to(timeline, story, { camera: 7 }, { camera: 8 }, 105.6, 9.6, EASE.camera);

  to(timeline, story, { dof: 0 }, { dof: 1 }, 13, 6, EASE.camera);
  to(timeline, story, { focus: 0 }, { focus: 1 }, 18.6, 5.2, EASE.camera);
  to(timeline, story, { dof: 1 }, { dof: 0 }, 30, 3.2, EASE.camera);

  to(timeline, story, { lamp: 1 }, { lamp: 0.8 }, 12, 6, EASE.camera);
  to(timeline, story, { lift: 0 }, { lift: 1 }, 34, 4.6, EASE.camera);
  to(timeline, story, { tubeOff: 0 }, { tubeOff: 1 }, 34.4, 2.6, EASE.out);
  to(timeline, story, { inside: 0 }, { inside: 1 }, 34.2, 4.2, EASE.camera);
  to(timeline, story, { sweep: 0 }, { sweep: 1 }, 39.6, 5, EASE.camera);
  to(timeline, story, { relief: 0 }, { relief: 1 }, 39.8, 4.6, EASE.camera);

  to(timeline, story, { place: 0 }, { place: 1 }, 52.6, 4.2, EASE.camera);

  to(timeline, story, { grab: 0 }, { grab: 1 }, 66, 0.9, EASE.in);
  to(timeline, story, { drag: 0 }, { drag: 1 }, 66.8, 3.6, EASE.camera);
  to(timeline, story, { grab: 1 }, { grab: 0 }, 70.3, 0.7, EASE.out);
  to(timeline, story, { place: 1 }, { place: 2 }, 70.7, 3.6, EASE.camera);
  to(timeline, story, { grab: 0 }, { grab: 1 }, 76.2, 0.9, EASE.in);
  to(timeline, story, { drag: 1 }, { drag: 0 }, 77, 3.6, EASE.camera);
  to(timeline, story, { grab: 1 }, { grab: 0 }, 80.5, 0.7, EASE.out);
  to(timeline, story, { place: 2 }, { place: 1 }, 80.9, 3.6, EASE.camera);

  to(timeline, story, { relief: 1 }, { relief: 0 }, 89.6, 2.6, EASE.camera);
  to(timeline, story, { sweep: 1 }, { sweep: 0 }, 89.6, 2.2, EASE.camera);
  to(timeline, story, { lift: 1 }, { lift: 0 }, 90.4, 4.6, EASE.camera);
  to(timeline, story, { inside: 1 }, { inside: 0 }, 91, 4.2, EASE.camera);
  to(timeline, story, { tubeOff: 1 }, { tubeOff: 0 }, 93.6, 0.05, 'none');
  to(timeline, story, { tubeOn: 1 }, { tubeOn: 0 }, 93.6, 0.05, 'none');
  to(timeline, story, { tubeOn: 0 }, { tubeOn: 1 }, 93.7, 2.2, EASE.out);
  to(timeline, story, { lamp: 0.8 }, { lamp: 0 }, 96.6, 0.35, 'power1.in');
  to(timeline, story, { frost: 0 }, { frost: 1 }, 98.4, 4.2, EASE.camera);
  to(timeline, story, { lamp: 0 }, { lamp: 0.55 }, 107, 5.5, EASE.camera);

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
    to(timeline, elements.navAction, { autoAlpha: 1, y: 0 }, { autoAlpha: 0, y: -8 }, 112.5, 2, EASE.out);
  }
  if (elements.skip) to(timeline, elements.skip, { autoAlpha: 1 }, { autoAlpha: 0 }, 115, 3, EASE.out);

  return { timeline, splits };
}

export function heroLines(element: HTMLElement) {
  return element.querySelectorAll<HTMLElement>('.line');
}
