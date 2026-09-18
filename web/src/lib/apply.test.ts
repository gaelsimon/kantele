import { describe, expect, it } from 'vitest';
import type { Apply } from './api';
import { costClass, dearest, shared } from './apply';

/// The server sends the mode and the sentence together, so a test that invents one invents both.
const SAYS: Record<Apply, string> = {
  never: 'You cannot change this setting on this page.',
  immediate: 'This change applies now.',
  'next-pass': 'This change applies at the next check.',
  reread: 'The server reads the library again.',
  restart: 'You must restart the server.',
};

const of = (...apply: Apply[]) =>
  apply.map((one) => ({ apply: one, says: SAYS[one] ?? '', tag: one }));

describe('what a save of some settings costs', () => {
  it('is nothing where nothing changed', () => {
    expect(dearest([])).toBeUndefined();
  });

  it('is the only mode where there is one', () => {
    expect(dearest(of('immediate'))?.apply).toBe('immediate');
  });

  it('is the dearest, whichever end it sits at', () => {
    expect(dearest(of('immediate', 'restart'))?.apply).toBe('restart');
    expect(dearest(of('restart', 'immediate'))?.apply).toBe('restart');
  });

  it('orders the modes the way the server does', () => {
    expect(dearest(of('never', 'immediate'))?.apply).toBe('immediate');
    expect(dearest(of('immediate', 'next-pass'))?.apply).toBe('next-pass');
    expect(dearest(of('next-pass', 'reread'))?.apply).toBe('reread');
    expect(dearest(of('reread', 'restart'))?.apply).toBe('restart');
  });

  it('holds a mode it has never heard of to be the dearest', () => {
    const unknown = { apply: 'something-new' as Apply, says: 'something new', tag: 'new' };
    expect(dearest([...of('restart'), unknown])).toBe(unknown);
    expect(dearest([unknown, ...of('immediate')])).toBe(unknown);
  });

  it('speaks in the words the server sent', () => {
    expect(dearest(of('immediate', 'reread'))?.says).toBe('The server reads the library again.');
    expect(dearest([{ apply: 'something-new' as Apply, says: 'something new', tag: 'new' }])?.says).toBe(
      'something new',
    );
  });
});

describe('what a section says beside its title', () => {
  it('is nothing for a section holding nothing', () => {
    expect(shared([])).toBeUndefined();
  });

  it('is the one cost where every key agrees', () => {
    expect(shared(of('reread', 'reread'))?.says).toBe('The server reads the library again.');
  });

  it('is the cost most keys share, so one dear key does not speak for four cheap ones', () => {
    expect(shared(of('restart', 'immediate', 'immediate', 'immediate', 'immediate'))?.apply).toBe(
      'immediate',
    );
  });

  it('is the dearest on a tie', () => {
    expect(shared(of('immediate', 'restart'))?.apply).toBe('restart');
  });
});

describe('the colour of a cost', () => {
  it('marks the three the eye should tell apart and leaves the rest plain', () => {
    expect(costClass('immediate')).toBe('now');
    expect(costClass('reread')).toBe('reread');
    expect(costClass('restart')).toBe('restart');
    expect(costClass('next-pass')).toBe('');
    expect(costClass('never')).toBe('');
  });
});
