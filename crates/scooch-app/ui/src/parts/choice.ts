export type Command = 'turn-on' | 'turn-off' | 'fix' | 'other';

export type Choice<A> = { pill: A | null; quiet: A | null };

export function choose<A>(failed: Command | null, fix: A | null, toggle: A | null, again: (command: 'turn-on' | 'turn-off') => A | null): Choice<A> {
  if (!failed) return { pill: fix ?? toggle, quiet: fix ? toggle : null };
  if (failed === 'turn-on' || failed === 'turn-off') return { pill: again(failed) ?? fix ?? toggle, quiet: null };
  if (failed === 'fix') return { pill: fix ?? toggle, quiet: null };
  return { pill: toggle ?? fix, quiet: null };
}
