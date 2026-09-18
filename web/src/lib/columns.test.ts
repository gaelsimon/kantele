import { describe, expect, it } from 'vitest';
import { above, halves, opened, shown, stepped, trailUnder } from './columns';

describe('the trail', () => {
  it('draws the top of the listing and then one column per folder opened', () => {
    expect(shown([])).toEqual(['']);
    expect(shown(['Blue Note', 'Blue Note/Sierra'])).toEqual(['', 'Blue Note', 'Blue Note/Sierra']);
  });

  it('closes what was open below the folder opened', () => {
    const deep = ['Blue Note', 'Blue Note/Sierra'];
    expect(opened(deep, 0, 'Autechre')).toEqual(['Autechre']);
    expect(opened(deep, 1, 'Blue Note/Kremerata')).toEqual(['Blue Note', 'Blue Note/Kremerata']);
    expect(opened(deep, 2, 'Blue Note/Sierra/Disc 2')).toEqual([...deep, 'Blue Note/Sierra/Disc 2']);
  });

  it('opens down to a path that is already chosen, and stops above it', () => {
    expect(trailUnder('/Volumes/music', '/Volumes/music/Rock/Live/01.flac')).toEqual([
      '/Volumes/music',
      '/Volumes/music/Rock',
      '/Volumes/music/Rock/Live',
    ]);
    expect(trailUnder('/Volumes/music', '/Volumes/music/Rock')).toEqual(['/Volumes/music']);
    expect(trailUnder('/Volumes/music', '/Volumes/music')).toEqual([]);
    expect(trailUnder('/Volumes/music', '/elsewhere/Rock')).toEqual([]);
  });

  it('knows the folder a path sits in', () => {
    expect(above('Blue Note/Sierra/01.flac')).toBe('Blue Note/Sierra');
    expect(above('Autechre')).toBe('');
  });
});

describe('a long name', () => {
  it('keeps its end, and gives way in the middle', () => {
    expect(halves('Animal Territory - Raumschmiere')).toEqual([
      'Animal Territory - ',
      'Raumschmiere',
    ]);
  });

  it('starts the end it keeps on a word where one is near the cut', () => {
    expect(halves("Tom's Diner (DNA Feat. Suzanne Vega)")).toEqual([
      "Tom's Diner (DNA Feat. ",
      'Suzanne Vega)',
    ]);
  });

  it('is left alone when it is not long', () => {
    expect(halves('Blue Note')).toEqual(['Blue Note', '']);
    expect(halves('Sierra Maestra 01')).toEqual(['Sierra Maestra 01', '']);
  });
});

describe('the arrow keys', () => {
  it('stop at the ends of the column', () => {
    expect(stepped(0, -1, 3)).toBe(0);
    expect(stepped(2, 1, 3)).toBe(2);
    expect(stepped(1, 1, 3)).toBe(2);
    expect(stepped(0, 1, 0)).toBe(0);
  });
});
