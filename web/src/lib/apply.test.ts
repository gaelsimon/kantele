import { describe, expect, it } from 'vitest';
import type { Apply } from './api';
import { badgeFor, dearest } from './apply';

/// The server sends the mode and the sentence together, so a test that invents one invents both.
const SAYS: Record<Apply, string> = {
  never: 'not written from this page',
  immediate: 'applies at once',
  'next-pass': 'applies at the next check',
  reread: 'reads the library again',
  restart: 'needs a restart',
};

const of = (...apply: Apply[]) =>
  apply.map((one) => ({ apply: one, says: SAYS[one] ?? 'not written from this page' }));

describe('what a block of settings costs', () => {
  it('is nothing where the block is empty', () => {
    expect(dearest([])).toBeUndefined();
  });

  it('is the only mode where there is one', () => {
    expect(dearest(of('immediate'))?.apply).toBe('immediate');
  });

  it('is the dearest, not the first', () => {
    expect(dearest(of('immediate', 'restart'))?.apply).toBe('restart');
  });

  it('is the dearest, whichever end it sits at', () => {
    expect(dearest(of('restart', 'immediate'))?.apply).toBe('restart');
  });

  it('orders the modes the way the server does', () => {
    expect(dearest(of('never', 'immediate'))?.apply).toBe('immediate');
    expect(dearest(of('immediate', 'next-pass'))?.apply).toBe('next-pass');
    expect(dearest(of('next-pass', 'reread'))?.apply).toBe('reread');
    expect(dearest(of('reread', 'restart'))?.apply).toBe('restart');
  });

  it('does not let a cheap first key speak for a dear second', () => {
    expect(badgeFor(of('immediate', 'reread'))).toBe('reads the library again');
  });
});

describe('the words beside a block', () => {
  it('are the ones the server sent for the dearest key', () => {
    expect(badgeFor(of('immediate'))).toBe('applies at once');
    expect(badgeFor(of('restart'))).toBe('needs a restart');
  });

  it('say so where nothing on this page can write the key', () => {
    expect(badgeFor(of('never'))).toBe('not written from this page');
  });

  it('are nothing at all for a block holding nothing', () => {
    expect(badgeFor([])).toBe('');
  });

  it('survive a mode this page has never heard of', () => {
    expect(badgeFor([{ apply: 'something-new' as Apply, says: 'something new' }])).toBe(
      'something new',
    );
  });
});
