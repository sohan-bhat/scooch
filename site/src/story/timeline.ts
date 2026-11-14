import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import type { DemoManifest } from './demo';
import { story } from './state';

gsap.registerPlugin(ScrollTrigger);

export const EASE = { camera: 'power2.inOut', in: 'power3.out', out: 'power2.in' } as const;
export const DURATION = { fadeIn: 2.2, fadeOut: 1.6 } as const;

export const TOTAL = 200;

export const beats = [
  { id: 'hero', start: 0, end: 12, rest: 2 },
  { id: 'problem', start: 12, end: 30, rest: 25.2 },
  { id: 'lift', start: 30, end: 50, rest: 45 },
  { id: 'fix', start: 50, end: 64, rest: 59.4 },
  { id: 'grow', start: 64, end: 78, rest: 74 },
  { id: 'follow', start: 78, end: 104, rest: 99.4 },
  { id: 'shapes', start: 104, end: 132, rest: 130.4 },
  { id: 'travel', start: 132, end: 156, rest: 153.6 },
  { id: 'macro', start: 156, end: 172, rest: 170.2 },
  { id: 'rest', start: 172, end: 186, rest: 183.4 },
  { id: 'end', start: 186, end: TOTAL, rest: 199.6 },
] as const;

export const captionTimes: Record<string, [number, number]> = {
  problem: [20, 28.6],
  fix: [55.4, 62],
  grow: [69.6, 76.2],
  shapes: [111.4, 131.4],
  rest: [179.4, 185.2],
};

export type Elements = {
  overlays: HTMLElement[];
  navAction: HTMLElement | null;
  skip: HTMLElement | null;
};

type Built = { timeline: gsap.core.Timeline };

function to(timeline: gsap.core.Timeline, target: object, from: gsap.TweenVars, end: gsap.TweenVars, at: number, duration: number, ease: string) {
  timeline.fromTo(target, from, { ...end, duration, ease, immediateRender: false }, at);
}

function fade(timeline: gsap.core.Timeline, element: HTMLElement | null | undefined, show: number, hide: number) {
  if (!element) return;
  if (show >= 0) {
    gsap.set(element, { autoAlpha: 0 });
    to(timeline, element, { autoAlpha: 0 }, { autoAlpha: 1 }, show, DURATION.fadeIn, EASE.in);
  }
  if (hide >= 0) to(timeline, element, { autoAlpha: 1 }, { autoAlpha: 0 }, hide, DURATION.fadeOut, EASE.out);
}

export function buildTimeline(elements: Elements, manifest: DemoManifest): Built {
  void manifest;
  const timeline = gsap.timeline({ paused: true, defaults: { ease: 'none' } });
  timeline.set({}, {}, TOTAL);
  const s = story;

  to(timeline, s, { camera: 0 }, { camera: 2 }, 8, 15.5, EASE.camera);
  to(timeline, s, { camera: 2 }, { camera: 3 }, 30, 4.4, EASE.camera);
  to(timeline, s, { camera: 3 }, { camera: 5 }, 34.2, 6.4, EASE.camera);
  to(timeline, s, { camera: 5 }, { camera: 6 }, 50, 8.6, EASE.camera);
  to(timeline, s, { camera: 6 }, { camera: 7 }, 64, 9, EASE.camera);
  to(timeline, s, { camera: 7 }, { camera: 8 }, 78, 12, EASE.camera);
  to(timeline, s, { camera: 8 }, { camera: 9 }, 104, 8.8, EASE.camera);
  to(timeline, s, { camera: 9 }, { camera: 10 }, 132, 9.2, EASE.camera);
  to(timeline, s, { camera: 10 }, { camera: 11 }, 142.6, 9.6, EASE.camera);
  to(timeline, s, { camera: 11 }, { camera: 12 }, 156, 4.6, EASE.camera);
  to(timeline, s, { camera: 12 }, { camera: 13 }, 160.6, 9.4, 'sine.inOut');
  to(timeline, s, { camera: 13 }, { camera: 14 }, 172, 8.8, EASE.camera);
  to(timeline, s, { camera: 14 }, { camera: 15 }, 186, 10, EASE.camera);

  to(timeline, s, { dof: 0 }, { dof: 1 }, 13, 6, EASE.camera);
  to(timeline, s, { focus: 0 }, { focus: 1 }, 18.6, 5.2, EASE.camera);
  to(timeline, s, { dof: 1 }, { dof: 0 }, 30, 3.2, EASE.camera);

  to(timeline, s, { lamp: 1 }, { lamp: 0.8 }, 12, 6, EASE.camera);
  to(timeline, s, { osd: 0 }, { osd: 1 }, captionTimes.problem[0], 1.2, 'steps(6)');
  to(timeline, s, { osd: 1 }, { osd: 0 }, captionTimes.problem[1], 0.6, 'none');

  to(timeline, s, { lift: 0 }, { lift: 1 }, 35.4, 4.6, EASE.camera);
  to(timeline, s, { tubeOff: 0 }, { tubeOff: 1 }, 35.8, 2.6, EASE.out);
  to(timeline, s, { inside: 0 }, { inside: 0.3 }, 35.6, 4.2, EASE.camera);
  to(timeline, s, { sweep: 0 }, { sweep: 1 }, 39.6, 5, EASE.camera);
  to(timeline, s, { relief: 0 }, { relief: 1 }, 39.8, 4.6, EASE.camera);

  to(timeline, s, { place: 0 }, { place: 1 }, 52.6, 4.2, EASE.camera);
  to(timeline, s, { typed: 0 }, { typed: 1 }, 53.8, 4, 'none');
  to(timeline, s, { place: 1 }, { place: 2 }, 66.2, 5.4, 'sine.inOut');
  to(timeline, s, { written: 0 }, { written: 1 }, 68.4, 4.2, 'sine.inOut');
  to(timeline, s, { relief: 1 }, { relief: 0.3 }, 65.4, 5, EASE.camera);

  to(timeline, s, { grab: 0 }, { grab: 1 }, 80, 0.9, EASE.in);
  to(timeline, s, { drag: 0 }, { drag: 1 }, 80.8, 3.6, EASE.camera);
  to(timeline, s, { grab: 1 }, { grab: 0 }, 84.3, 0.7, EASE.out);
  to(timeline, s, { place: 2 }, { place: 3 }, 84.7, 3.8, EASE.camera);
  to(timeline, s, { grab: 0 }, { grab: 1 }, 90.2, 0.9, EASE.in);
  to(timeline, s, { drag: 1 }, { drag: 0 }, 91, 3.6, EASE.camera);
  to(timeline, s, { grab: 1 }, { grab: 0 }, 94.5, 0.7, EASE.out);
  to(timeline, s, { place: 3 }, { place: 2 }, 94.9, 3.8, EASE.camera);

  to(timeline, s, { relief: 0.3 }, { relief: 0 }, 104, 3, EASE.camera);
  to(timeline, s, { wall: 0 }, { wall: 1 }, 108.6, 4, EASE.camera);
  to(timeline, s, { shape: 0 }, { shape: 1 }, 113.4, 3.6, EASE.camera);
  to(timeline, s, { shape: 1 }, { shape: 2 }, 119.2, 3.8, EASE.camera);
  to(timeline, s, { shape: 2 }, { shape: 3 }, 125.2, 3.6, EASE.camera);
  to(timeline, s, { wall: 1 }, { wall: 0 }, 131.6, 3, EASE.camera);

  to(timeline, s, { sweep: 1 }, { sweep: 0 }, 132, 2.2, EASE.camera);
  to(timeline, s, { fly: 0 }, { fly: 1 }, 132.6, 8.4, EASE.camera);
  to(timeline, s, { inside: 0.3 }, { inside: 0 }, 102.4, 5.2, EASE.camera);
  to(timeline, s, { city: 0 }, { city: 1 }, 134.4, 0.05, 'none');
  to(timeline, s, { tubeOff: 1 }, { tubeOff: 0 }, 134.4, 0.05, 'none');
  to(timeline, s, { tubeOn: 1 }, { tubeOn: 0 }, 134.4, 0.05, 'none');
  to(timeline, s, { tubeOn: 0 }, { tubeOn: 1 }, 134.5, 2.2, EASE.out);
  to(timeline, s, { fly: 1 }, { fly: 2 }, 143, 9.2, EASE.camera);

  to(timeline, s, { drift: 0 }, { drift: 1 }, 158, 14, 'sine.inOut');
  to(timeline, s, { lamp: 0.8 }, { lamp: 0 }, 178.2, 0.35, 'power1.in');
  to(timeline, s, { lamp: 0 }, { lamp: 0.55 }, 188, 5.5, EASE.camera);

  const [hero, end, ...rest] = elements.overlays;
  fade(timeline, hero, -1, 6.4);
  fade(timeline, end, 192.2, -1);
  const ids = Object.keys(captionTimes);
  rest.forEach((element, index) => {
    const times = captionTimes[ids[index]];
    if (times) fade(timeline, element, times[0], times[1]);
  });

  if (elements.navAction) {
    gsap.set(elements.navAction, { autoAlpha: 0 });
    to(timeline, elements.navAction, { autoAlpha: 0 }, { autoAlpha: 1 }, 9, 2, EASE.in);
    to(timeline, elements.navAction, { autoAlpha: 1 }, { autoAlpha: 0 }, 192.4, 2, EASE.out);
  }
  if (elements.skip) to(timeline, elements.skip, { autoAlpha: 1 }, { autoAlpha: 0 }, 195, 3, EASE.out);

  return { timeline };
}
