// What a change to a setting costs, in the server's own order and the server's own words.

import type { Apply, Setting } from './api';

/// Increasing cost, as `config::Apply` orders them.
const COST: Apply[] = ['never', 'immediate', 'next-pass', 'reread', 'restart'];

type Costed = Pick<Setting, 'apply' | 'says' | 'tag'>;

/// A mode this page has never heard of is the dearest, never the cheapest: a save that needs a
/// restart must not be announced as costing nothing.
const rank = (apply: Apply) => {
  const at = COST.indexOf(apply);
  return at === -1 ? COST.length : at;
};

/// The dearest of some settings, which is what one save of them all comes to.
export function dearest(settings: Costed[]): Costed | undefined {
  return settings.reduce<Costed | undefined>(
    (held, setting) => (held === undefined || rank(setting.apply) > rank(held.apply) ? setting : held),
    undefined,
  );
}

/// The cost most of a section's keys share, the dearest on a tie: what the section's badge says.
/// A key that costs otherwise carries its own tag beside the badge.
export function shared(settings: Costed[]): Costed | undefined {
  const counted = new Map<Apply, { held: Costed; times: number }>();
  for (const setting of settings) {
    const seen = counted.get(setting.apply);
    counted.set(setting.apply, { held: seen?.held ?? setting, times: (seen?.times ?? 0) + 1 });
  }
  let chosen: { held: Costed; times: number } | undefined;
  for (const one of counted.values()) {
    if (
      chosen === undefined ||
      one.times > chosen.times ||
      (one.times === chosen.times && rank(one.held.apply) > rank(chosen.held.apply))
    ) {
      chosen = one;
    }
  }
  return chosen?.held;
}

/// The page's class for a cost, which colours the badge.
export function costClass(apply: Apply): string {
  switch (apply) {
    case 'immediate':
      return 'now';
    case 'reread':
      return 'reread';
    case 'restart':
      return 'restart';
    default:
      return '';
  }
}
