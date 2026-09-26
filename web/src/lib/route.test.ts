import { describe, expect, it } from 'vitest';
import { href, parse } from './route';

describe('an address', () => {
  it('names a tab, or leaves it to the page', () => {
    expect(parse('/config/settings', '').tab).toBe('settings');
    expect(parse('/config/library', '').tab).toBe('library');
    expect(parse('/config', '').tab).toBeNull();
    expect(parse('/config/elsewhere', '').tab).toBeNull();
  });

  it('carries a folder or a file of the library, spelled as the folder spells it', () => {
    const route = { tab: 'library' as const, path: 'Neil Young/Harvest #2 & more/05 100%.flac' };
    expect(parse(href(route), '').path).toBe(route.path);
    expect(href({ tab: 'library', path: 'Neil Young/Harvest' })).toBe('/config/library/Neil%20Young/Harvest');
  });

  it('carries the checks ticked, and only on the library', () => {
    expect(href({ tab: 'library', only: ['no-genre', 'no-cover'] })).toBe('/config/library?only=no-genre,no-cover');
    expect(parse('/config/library', '?only=no-genre,no-cover').only).toEqual(['no-genre', 'no-cover']);
    expect(href({ tab: 'settings', only: ['no-genre'] })).toBe('/config/settings');
    expect(parse('/config/settings', '?only=no-genre').only).toEqual([]);
  });

  it('keeps no empty part, whatever slashes the address carries', () => {
    expect(parse('/config/library//Blue%20Note/', '').path).toBe('Blue Note');
  });
});
