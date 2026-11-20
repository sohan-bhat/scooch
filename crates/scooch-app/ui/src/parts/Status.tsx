import { Button } from '@/components/ui/button';
import type { Problem, View } from '~/backend/types';
import { copy, fill } from '~/lib/copy';

export type Fix = { label: string; run: () => void };
export type Said = { line: string; fix: Fix | null };

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
      return { line: copy.problem.gnome.line, fix: fix(copy.problem.gnome.fix) };
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

export function said(view: View, act: (problem: Problem) => void, pick: () => void, flip: () => void): Said {
  if (view.problem) return problemLine(view.problem, act, pick);
  if (view.phase === 'starting') return { line: copy.status.starting, fix: null };
  if (view.phase === 'stopping') return { line: copy.status.stopping, fix: null };
  if (view.phase === 'off') {
    if (view.pending === 1) return { line: copy.status.pending.one, fix: null };
    if (view.pending > 1) return { line: fill(copy.status.pending.many, { n: view.pending }), fix: null };
    return { line: copy.status.off, fix: null };
  }
  if (view.flipHelps && view.switches.flip === false) return { line: copy.hint.flip.line, fix: { label: copy.hint.flip.fix, run: flip } };
  return { line: copy.status.on, fix: null };
}

export function Status({ words, pill }: { words: Said; pill: { label: string; busy: boolean; run: () => void } | null }) {
  return (
    <div className="flex min-h-16 items-center gap-x-4 gap-y-2 px-6 cramped:min-h-14 cramped:px-4">
      <p key={words.line} role="status" aria-live="polite" className="line-in m-0 min-w-0 text-[1.125rem] leading-snug font-semibold text-paper cramped:text-base">
        {words.line}
      </p>
      {words.fix && (
        <Button variant="quiet" size="sm" onClick={words.fix.run} className="cramped:h-9 cramped:px-4 cramped:text-[0.875rem]">
          {words.fix.label}
        </Button>
      )}
      {pill && (
        <Button size="md" onClick={pill.run} disabled={pill.busy} className="ml-auto min-w-[8.5rem] cramped:h-10 cramped:min-w-[7rem] cramped:px-5 cramped:text-[0.9375rem]">
          {pill.label}
        </Button>
      )}
    </div>
  );
}
