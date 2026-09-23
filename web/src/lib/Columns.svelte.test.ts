// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import Columns from './Columns.svelte';
import type { Level } from './columns';

const held: Record<string, string[]> = {
  '': ['Blue Note', 'Kremerata'],
  'Blue Note': ['Blue Note/Blakey'],
};

async function load(path: string): Promise<Level<null>> {
  return {
    entries: (held[path] ?? []).map((at) => ({
      path: at,
      name: at.split('/').pop() ?? at,
      folder: true,
      of: null,
    })),
    more: false,
  };
}

const settled = () => new Promise((done) => setTimeout(done, 20));

describe('the columns', () => {
  afterEach(() => cleanup());

  it('keep the folder an owner opened when the library is published again', async () => {
    // Owned as a parent owns it, so a new `reload` is the only prop that moves.
    const props = $state({ load, reload: 0, onpick: () => {} });
    render(Columns, { props });
    await waitFor(() => expect(screen.getByTitle('Blue Note')).toBeTruthy());
    await fireEvent.click(screen.getByTitle('Blue Note'));
    await waitFor(() => expect(screen.getByTitle('Blue Note/Blakey')).toBeTruthy());

    props.reload = 1;
    await settled();
    expect(
      screen.queryByTitle('Blue Note/Blakey'),
      'a check or a saved setting publishes again, and the owner stays where they were',
    ).toBeTruthy();
  });
});
