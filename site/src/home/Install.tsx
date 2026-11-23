import { DownloadSimple } from '@phosphor-icons/react/dist/ssr';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import content from '@/content.json';

type Arch = 'x64' | 'arm64';
type Hints = { platform?: string; getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string; platform?: string }> };

function systemOf(agent: string) {
  if (/Windows/i.test(agent)) return 'windows';
  if (/Linux|X11|CrOS/i.test(agent) && !/Android/i.test(agent)) return 'linux';
  return 'macos';
}

async function guess(): Promise<{ system: string; arch: Arch; kind: string }> {
  const hints = (navigator as Navigator & { userAgentData?: Hints }).userAgentData;
  const agent = `${navigator.userAgent} ${hints?.platform ?? ''}`;
  const system = systemOf(agent);
  let arch: Arch = /aarch64|arm64|armv8/i.test(navigator.userAgent) ? 'arm64' : 'x64';
  const found = await hints?.getHighEntropyValues?.(['architecture']).catch(() => undefined);
  if (found?.architecture === 'arm') arch = 'arm64';
  if (found?.architecture === 'x86') arch = 'x64';
  const kind = /Fedora|Red Hat|openSUSE|SUSE/i.test(navigator.userAgent) ? 'rpm' : 'deb';
  return { system, arch, kind };
}

function click() {
  void import('@/lib/audio').then((audio) => audio.shot('click', { gain: 0.45 }));
}

export function Install() {
  const { install } = content;
  const [system, setSystem] = useState('macos');
  const [arch, setArch] = useState<Arch>('x64');
  const [kind, setKind] = useState('deb');
  useEffect(() => {
    guess().then((found) => {
      setSystem(found.system);
      setArch(found.arch);
      setKind(found.kind);
    });
  }, []);
  return (
    <section id="install" className="page-gutter relative scroll-mt-20 pt-[clamp(6rem,14vw,11rem)] pb-[clamp(5rem,10vw,8rem)]" aria-labelledby="install-title">
      <div className="grid gap-10 wide:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] wide:items-end wide:gap-[6vw]">
        <h2 id="install-title" data-reveal className="text-(length:--text-beat) text-paper">
          {install.title}
        </h2>
        <Tabs value={system} onValueChange={setSystem} data-reveal style={{ '--reveal-index': 1 } as React.CSSProperties} className="min-w-0 gap-6">
          <TabsList aria-label="Operating system">
            {install.systems.map((item) => (
              <TabsTrigger key={item.id} value={item.id}>
                {item.label}
              </TabsTrigger>
            ))}
          </TabsList>
          {install.systems.map((item) => {
            const fits = item.downloads.filter((download) => download.arch === arch || download.arch === 'any');
            const main = fits.find((download) => download.kind === kind) ?? fits[0] ?? item.downloads[0];
            const same = item.downloads.filter((download) => download !== main && download.arch === main.arch);
            const other = item.downloads.filter((download) => download.arch !== main.arch);
            const otherArch = other[0]?.arch as Arch | undefined;
            return (
              <TabsContent key={item.id} value={item.id} tabIndex={-1} className="flex flex-col items-start gap-5 data-[state=active]:animate-[tip-in_240ms_var(--ease-out)]">
                <Button asChild size="lg">
                  <a href={`${install.release}${main.file}`} download onClick={click}>
                    <DownloadSimple weight="bold" aria-hidden />
                    {main.label}
                  </a>
                </Button>
                {(same.length > 0 || otherArch) && (
                  <nav aria-label={install.others} className="flex flex-wrap items-center gap-2.5">
                    {same.map((download) => (
                      <Button key={download.file} asChild variant="quiet" size="sm">
                        <a href={`${install.release}${download.file}`} download aria-label={download.name} onClick={click}>
                          {download.format}
                        </a>
                      </Button>
                    ))}
                    {otherArch && other.length === 1 && (
                      <Button asChild variant="quiet" size="sm">
                        <a href={`${install.release}${other[0].file}`} download aria-label={other[0].name} onClick={click}>
                          {install.processors[otherArch]}
                        </a>
                      </Button>
                    )}
                    {otherArch && other.length > 1 && (
                      <Button variant="quiet" size="sm" onClick={() => setArch(otherArch)}>
                        {install.processors[otherArch]}
                      </Button>
                    )}
                  </nav>
                )}
              </TabsContent>
            );
          })}
        </Tabs>
      </div>
    </section>
  );
}
