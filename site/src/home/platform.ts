import { useEffect, useState } from 'react';
import content from '@/content.json';

export type Arch = 'x64' | 'arm64';
export type Platform = { system: string; arch: Arch; kind: string };
type Hints = { platform?: string; getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string; platform?: string }> };
type System = (typeof content.install.systems)[number];

const start: Platform = { system: 'macos', arch: 'x64', kind: 'deb' };
let found: Promise<Platform> | undefined;

function systemOf(agent: string) {
  if (/Windows/i.test(agent)) return 'windows';
  if (/Linux|X11|CrOS/i.test(agent) && !/Android/i.test(agent)) return 'linux';
  return 'macos';
}

async function guess(): Promise<Platform> {
  const hints = (navigator as Navigator & { userAgentData?: Hints }).userAgentData;
  const agent = `${navigator.userAgent} ${hints?.platform ?? ''}`;
  const system = systemOf(agent);
  let arch: Arch = /aarch64|arm64|armv8/i.test(navigator.userAgent) ? 'arm64' : 'x64';
  const high = await hints?.getHighEntropyValues?.(['architecture']).catch(() => undefined);
  if (high?.architecture === 'arm') arch = 'arm64';
  if (high?.architecture === 'x86') arch = 'x64';
  const kind = /Fedora|Red Hat|openSUSE|SUSE/i.test(navigator.userAgent) ? 'rpm' : 'deb';
  return { system, arch, kind };
}

export function usePlatform() {
  const [platform, setPlatform] = useState(start);
  useEffect(() => {
    let live = true;
    found ??= guess();
    void found.then((value) => {
      if (live) setPlatform(value);
    });
    return () => {
      live = false;
    };
  }, []);
  return [platform, setPlatform] as const;
}

export function choose(item: System, arch: Arch, kind: string) {
  const fits = item.downloads.filter((download) => download.arch === arch || download.arch === 'any');
  return fits.find((download) => download.kind === kind) ?? fits[0] ?? item.downloads[0];
}

export function click() {
  void import('@/lib/audio').then((audio) => audio.shot('click', { gain: 0.45 }));
}
