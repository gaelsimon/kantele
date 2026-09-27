import { describe, expect, it } from 'vitest';
import { FALLBACK_NAME, screen, type Holdings } from './screen';

const LIBRARY: Holdings = { albums: 355, tracks: 511, untagged: 0, playlists: 0, compilations: 0 };
const labels = (axes: string[], recent: number, holds = LIBRARY) =>
  screen('', axes, recent, holds).rows.map((row) => row.label);

describe('a player screen', () => {
  it('shows the fallback name where the owner typed none', () => {
    expect(screen('', [], 0, LIBRARY).name).toBe(FALLBACK_NAME);
    expect(screen('   ', [], 0, LIBRARY).name).toBe(FALLBACK_NAME);
    expect(screen('Salon', [], 0, LIBRARY).name).toBe('Salon');
  });

  it('leads with the albums and ends with every track, named without their count', () => {
    expect(labels([], 0)).toEqual(['Albums', 'Folders', 'Tracks']);
  });

  it('leaves out an axis the server said narrows nothing', () => {
    expect(
      screen('', ['Artists', 'Genres', 'Audio quality'], 0, LIBRARY, ['Audio quality']).rows.map(
        (row) => row.label,
      ),
    ).toEqual(['Albums', 'Artists', 'Genres', 'Folders', 'Tracks']);
  });

  it('draws every axis while nothing has been said about them', () => {
    expect(labels(['Genres', 'Audio quality'], 0)).toContain('Audio quality');
  });

  it('offers the axes in their own order, whatever the library holds', () => {
    expect(labels(['Genres', 'Artists'], 0)).toEqual([
      'Albums',
      'Genres',
      'Artists',
      'Folders',
      'Tracks',
    ]);
    expect(
      labels(['Genres'], 0, { albums: 2, tracks: 9, untagged: 0, playlists: 0, compilations: 0 }),
    ).toContain('Genres');
  });

  it('offers Recently added only above zero, after the menus', () => {
    expect(labels(['Genres'], 300)).toEqual([
      'Albums',
      'Genres',
      'Recently added',
      'Folders',
      'Tracks',
    ]);
    expect(labels(['Genres'], 0)).not.toContain('Recently added');
  });

  it('shows compilations, playlists and untagged tracks only where the library has some', () => {
    const some = { ...LIBRARY, untagged: 3, playlists: 2, compilations: 4 };
    expect(labels([], 0, some)).toEqual([
      'Albums',
      'Compilations',
      'Playlists',
      'Folders',
      'Tracks',
      'Untagged tracks',
    ]);
  });

  it('is nearly bare for an empty library', () => {
    expect(
      labels(['Genres'], 300, { albums: 0, tracks: 0, untagged: 0, playlists: 0, compilations: 0 }),
    ).toEqual(['Tracks']);
  });

  it('marks as fixed every entry that is not a setting', () => {
    const rows = screen('', ['Genres'], 1, { ...LIBRARY, playlists: 1 }).rows;
    expect(rows.filter((row) => !row.fixed).map((row) => row.label)).toEqual([
      'Genres',
      'Recently added',
    ]);
  });
});
