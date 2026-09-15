// What a change to a setting costs, in the server's own order and the server's own words.

import type { Apply, Setting } from './api';

/// Increasing cost, as `config::Apply` orders them.
const COST: Apply[] = ['never', 'immediate', 'next-pass', 'reread', 'restart'];

type Costed = Pick<Setting, 'apply' | 'says'>;

/// The dearest of some settings, which is what a block of them costs. A block badged by its first
/// key alone lies as soon as the block holds two keys that differ.
export function dearest(settings: Costed[]): Costed | undefined {
  return settings.reduce<Costed | undefined>(
    (held, setting) =>
      held === undefined || COST.indexOf(setting.apply) > COST.indexOf(held.apply) ? setting : held,
    undefined,
  );
}

/// What a block of settings says beside its title.
export function badgeFor(settings: Costed[]): string {
  return dearest(settings)?.says ?? '';
}
