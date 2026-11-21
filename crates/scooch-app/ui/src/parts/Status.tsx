import { Button } from '@/components/ui/button';
import type { Problem, View } from '~/backend/types';
import { copy, fill } from '~/lib/copy';

export type Act = { label: string; run: () => void; busy?: boolean };
export type Said = { line: string; fix: Act | null };

export function problemLine(problem: Problem, act: (problem: Problem) => void, pick: () => void): Said {
  const fix = (label: string) => ({ label, run: () => act(problem) });
  switch (problem.kind) {
    case 'automation':
      return { line: copy.problem.automation.line, fix: fix(copy.problem.automation.fix) };
    case 'asking':
      return { line: copy.problem.asking.line, fix: null };
    case 'finder':
      return { line: copy.problem.finder.line, fix: fix(copy.problem.finder.fix) };
    case 'move':
      return { line: copy.problem.move.line, fix: fix(copy.problem.move.fix) };
    case 'wallpaper':
      return { line: copy.problem.wallpaper[problem.what], fix: { label: copy.problem.wallpaper.fix, run: pick } };
    case 'gnome':
      return problem.icons === 'off' ? { line: copy.problem.gnome.line, fix: fix(copy.problem.gnome.fix) } : { line: copy.problem.gnome[problem.icons], fix: null };
    case 'unknown':
      return { line: fill(copy.problem.unknown.line, { desktop: problem.desktop }), fix: fix(fill(copy.problem.unknown.fix, { desktop: problem.desktop })) };
    case 'admin':
      return { line: copy.problem.admin.line, fix: fix(copy.problem.admin.fix) };
    case 'startup':
      return { line: copy.problem.startup.line, fix: fix(copy.problem.startup.fix) };
    case 'stopped':
      return { line: copy.problem.stopped.line, fix: fix(copy.problem.stopped.fix) };
    case 'screen':
      return { line: copy.problem.screen.line, fix: null };
    case 'other':
      return { line: copy.problem.other.line, fix: fix(copy.problem.other.fix) };
  }
}

function onLine(view: View) {
  const counted = view.counted;
  if (!counted) return copy.status.on.plain;
  if (counted.icons === 0) return copy.status.on.none;
  if (counted.icons === 1) return copy.status.on.one;
  if (counted.screens > 1) return fill(copy.status.on.spread, { n: counted.icons, screens: counted.screens });
  return fill(copy.status.on.many, { n: counted.icons });
}

export function said(view: View, act: (problem: Problem) => void, pick: () => void, flip: () => void): Said {
  const failure = view.failure ? (copy.failure as Record<string, string>)[view.failure] ?? copy.failure.other : null;
  if (view.problem) {
    const problem = problemLine(view.problem, act, pick);
    return failure ? { line: failure, fix: problem.fix } : problem;
  }
  if (failure) return { line: failure, fix: null };
  if (view.phase === 'starting') return { line: copy.status.starting, fix: null };
  if (view.phase === 'stopping') return { line: copy.status.stopping, fix: null };
  if (view.phase === 'off') {
    if (view.pending === 1) return { line: copy.status.pending.one, fix: null };
    if (view.pending > 1) return { line: fill(copy.status.pending.many, { n: view.pending }), fix: null };
    return { line: copy.status.off, fix: null };
  }
  if (view.flipHelps && view.switches.flip === false) return { line: copy.hint.flip.line, fix: { label: copy.hint.flip.fix, run: flip } };
  return { line: onLine(view), fix: null };
}

export function Status({ line, pill, quiet }: { line: string; pill: Act | null; quiet: Act | null }) {
  return (
    <div className="flex min-h-[64px] flex-wrap items-center gap-x-[16px] gap-y-[8px] px-[24px] py-[8px] cramped:min-h-[56px] cramped:px-[16px]">
      <p key={line} role="status" aria-live="polite" className="line-in m-0 min-w-0 text-[1.125rem] leading-snug font-semibold text-paper cramped:text-base">
        {line}
      </p>
      {quiet && (
        <Button variant="quiet" size="sm" onClick={quiet.busy ? undefined : quiet.run} aria-disabled={quiet.busy || undefined} className="h-auto min-h-[40px] shrink-0 px-[20px] py-[6px] aria-disabled:pointer-events-none cramped:min-h-[36px] cramped:px-[16px] cramped:text-[0.875rem]">
          {quiet.label}
        </Button>
      )}
      {pill && (
        <Button
          size="md"
          onClick={pill.busy ? undefined : pill.run}
          aria-disabled={pill.busy || undefined}
          className="ml-auto h-auto min-h-[48px] min-w-[136px] px-[24px] py-[8px] aria-disabled:pointer-events-none cramped:min-h-[40px] cramped:min-w-[112px] cramped:px-[20px] cramped:text-[0.9375rem]"
        >
          {pill.label}
        </Button>
      )}
    </div>
  );
}
