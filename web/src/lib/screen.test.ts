import { describe, expect, it } from 'vitest';
import { FALLBACK_NAME, screen, type Holdings } from './screen';

const LIBRARY: Holdings = { albums: 355, tracks: 511, untagged: 0, playlists: 0 };
const labels = (axes: string[], recent: number, holds = LIBRARY) =>
  screen('', axes, recent, holds).rows.map((row) => row.label);

describe('a player screen', () => {
  it('shows the fallback name where the owner typed none', () => {
    expect(screen('', [], 0, LIBRARY).name).toBe(FALLBACK_NAME);
    expect(screen('   ', [], 0, LIBRARY).name).toBe(FALLBACK_NAME);
    expect(screen('Salon', [], 0, LIBRARY).name).toBe('Salon');
  });

  it('leads with the albums and every item, counted the way the wire titles them', () => {
    expect(labels([], 0)).toEqual(['355 albums', '511 items', '[folder view]']);
    expect(labels([], 0, { albums: 1, tracks: 1, untagged: 0, playlists: 0 })).toEqual([
      '1 album',
      '1 item',
      '[folder view]',
    ]);
  });

  it('leaves out an axis the server said narrows nothing', () => {
    expect(
      screen('', ['Genre', 'Artist', 'Quality'], 0, LIBRARY, ['Quality']).rows.map(
        (row) => row.label,
      ),
    ).toEqual(['355 albums', '511 items', 'Genre', 'Artist', '[folder view]']);
  });

  it('draws every axis while nothing has been said about them', () => {
    expect(labels(['Genre', 'Quality'], 0)).toContain('Quality');
  });

  it('offers the axes in their own order, whatever the library holds', () => {
    expect(labels(['Genre', 'Artist'], 0)).toEqual([
      '355 albums',
      '511 items',
      'Genre',
      'Artist',
      '[folder view]',
    ]);
    expect(labels(['Genre'], 0, { albums: 2, tracks: 9, untagged: 0, playlists: 0 })).toContain('Genre');
  });

  it('offers Recently added only above zero, before the folder view', () => {
    expect(labels(['Genre'], 300)).toEqual([
      '355 albums',
      '511 items',
      'Genre',
      'Recently added',
      '[folder view]',
    ]);
    expect(labels(['Genre'], 0)).not.toContain('Recently added');
  });

  it('shows untagged and playlists only where the library has some', () => {
    const some = { ...LIBRARY, untagged: 3, playlists: 2 };
    expect(labels([], 0, some)).toEqual([
      '355 albums',
      '511 items',
      '[untagged]',
      'Playlists',
      '[folder view]',
    ]);
  });

  it('is nearly bare for an empty library', () => {
    expect(labels(['Genre'], 300, { albums: 0, tracks: 0, untagged: 0, playlists: 0 })).toEqual([
      '0 items',
    ]);
  });

  it('marks as fixed every entry that is not a setting', () => {
    const rows = screen('', ['Genre'], 1, { ...LIBRARY, playlists: 1 }).rows;
    expect(rows.filter((row) => !row.fixed).map((row) => row.label)).toEqual([
      'Genre',
      'Recently added',
    ]);
  });
});

