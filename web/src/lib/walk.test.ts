import { describe, expect, it } from 'vitest';
import { Walk } from './walk.svelte';
import type { Entry, Level } from './columns';

const folder = (path: string): Entry<null> => ({
  path,
  name: path.split('/').pop() ?? path,
  folder: true,
  of: null,
});

const settled = () => new Promise((done) => setTimeout(done, 0));

/// A tree in memory, whose reads the test settles one at a time.
function tree(held: Record<string, string[]>) {
  type Asking = { path: string; settle: (level: Level<null>) => void; fail: (why: Error) => void };
  const asking: Asking[] = [];
  const walk = new Walk<null>(
    (path) =>
      new Promise<Level<null>>((settle, fail) => {
        asking.push({ path, settle, fail });
      }),
  );

  /// Waits for the read to be asked for, then answers it.
  async function answer(path: string, why?: Error) {
    for (let tries = 0; tries < 50; tries += 1) {
      const at = asking.findIndex((one) => one.path === path);
      if (at >= 0) {
        const [one] = asking.splice(at, 1);
        if (why) one?.fail(why);
        else one?.settle({ entries: (held[path] ?? []).map(folder), more: false });
        return settled();
      }
      await settled();
    }
    throw new Error(`nothing asked for ${path}`);
  }

  return { walk, asking, answer };
}

describe('walking a tree', () => {
  it('opens a folder and closes what was open below it', async () => {
    const { walk, answer } = tree({ '': ['a', 'b'], a: ['a/one'], b: [] });
    const reading = walk.restart();
    await answer('');
    await reading;

    walk.open(0, 'a');
    await answer('a');
    expect(walk.columns).toEqual(['', 'a']);

    walk.open(1, 'a/one');
    expect(walk.columns).toEqual(['', 'a', 'a/one']);

    walk.open(0, 'b');
    expect(walk.columns).toEqual(['', 'b']);
  });

  it('closes every column past a file that was picked', async () => {
    const { walk, answer } = tree({ '': ['a'], a: ['a/one'] });
    const reading = walk.restart();
    await answer('');
    await reading;
    walk.open(0, 'a');
    await answer('a');
    expect(walk.columns).toHaveLength(2);
    walk.close(1);
    expect(walk.columns).toEqual(['', 'a']);
    walk.close(0);
    expect(walk.columns).toEqual(['']);
  });

  it('drops an answer that arrives after a later one was asked for', async () => {
    const { walk, asking } = tree({});
    void walk.read('a');
    void walk.read('a');
    const [first, second] = asking;
    second?.settle({ entries: [folder('a/late')], more: false });
    first?.settle({ entries: [folder('a/early')], more: false });
    await settled();
    expect(walk.entries('a').map((one) => one.path)).toEqual(['a/late']);
  });

  it('remembers why a column would not read, and forgets it once it does', async () => {
    const { walk, answer } = tree({ a: ['a/one'] });
    const failing = walk.read('a');
    await answer('a', new Error('Failed to fetch'));
    await failing;
    expect(walk.broke.a).toBe('Failed to fetch');
    expect(walk.entries('a')).toEqual([]);

    const reading = walk.read('a');
    await answer('a');
    await reading;
    expect(walk.broke.a).toBeUndefined();
    expect(walk.entries('a')).toHaveLength(1);
  });

  it('opens down to the path it is given, and stops above it', async () => {
    const { walk, answer } = tree({
      '': ['/music'],
      '/music': ['/music/rock'],
      '/music/rock': ['/music/rock/live'],
    });
    const reading = walk.restart();
    await answer('');
    await reading;

    const revealing = walk.revealTo('/music/rock/live');
    await answer('/music');
    await answer('/music/rock');
    await revealing;
    expect(walk.columns).toEqual(['', '/music', '/music/rock']);
  });

  it('opens down to nothing when the path is under no share it knows', async () => {
    const { walk, answer } = tree({ '': ['/music'] });
    const reading = walk.restart();
    await answer('');
    await reading;
    await walk.revealTo('/elsewhere/rock');
    expect(walk.columns).toEqual(['']);
  });
});
