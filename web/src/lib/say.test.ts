import { describe, expect, it } from 'vitest';
import { ago, extension, listed, many, noun, tagName, took, uptime } from './say';

describe('a count and the noun beside it', () => {
  it('is singular where there is one', () => {
    expect(many(1, 'album')).toBe('1 album');
  });

  it('is plural at anything else', () => {
    expect(many(21, 'album')).toBe('21 albums');
  });

  it('is plural at none, which is how English counts nothing', () => {
    expect(many(0, 'problem')).toBe('0 problems');
  });

  it('takes a plural that is not the singular and an s', () => {
    expect(many(2, 'this', 'these')).toBe('2 these');
  });

  it('answers with the noun alone where the number is shown apart', () => {
    expect(noun(1, 'playlist')).toBe('playlist');
  });
});

describe('how long a server has been up', () => {
  it('counts seconds under a minute', () => {
    expect(uptime(42)).toBe('42 s');
  });

  it('drops to minutes past one', () => {
    expect(uptime(90)).toBe('1 min');
  });

  it('pairs hours with minutes', () => {
    expect(uptime(3_600 + 120)).toBe('1 h 2 min');
  });

  it('pairs days with hours, and stops there', () => {
    expect(uptime(86_400 * 2 + 3_600 * 3 + 59)).toBe('2 d 3 h');
  });

  it('says zero rather than nothing', () => {
    expect(uptime(0)).toBe('0 s');
  });
});

describe('how long a pass took', () => {
  it('is under a second where it was', () => {
    expect(took(0.4)).toBe('under a second');
  });

  it('keeps a decimal while the number is small', () => {
    expect(took(2.3)).toBe('2.3 s');
  });

  it('drops the decimal once it stops mattering', () => {
    expect(took(42.7)).toBe('43 s');
  });

  it('turns to minutes past a minute and a half', () => {
    expect(took(600)).toBe('10 min');
  });

  it('turns to hours past ninety minutes of them', () => {
    expect(took(3_600 * 3)).toBe('3 h');
  });

  /// A boundary either side, since a pass of exactly this length is the one that reads wrong.
  it('changes unit at ninety seconds, not before', () => {
    expect(took(89)).toBe('89 s');
    expect(took(90)).toBe('2 min');
  });
});

describe('how long ago something happened', () => {
  const now = 1_000_000;

  it('calls the last minute just now', () => {
    expect(ago(now - 30, now)).toBe('just now');
  });

  it('counts minutes, then hours, then days', () => {
    expect(ago(now - 600, now)).toBe('10 min ago');
    expect(ago(now - 7_200, now)).toBe('2 h ago');
    expect(ago(now - 86_400 * 3, now)).toBe('3 d ago');
  });

  /// A clock that disagrees with the server's is the ordinary case, not a broken one.
  it('does not count backwards where the reader clock is behind', () => {
    expect(ago(now + 500, now)).toBe('just now');
  });
});

describe('a list in prose', () => {
  it('is nothing where there is nothing', () => {
    expect(listed([])).toBe('');
  });

  it('is the one thing where there is one', () => {
    expect(listed(['a date'])).toBe('a date');
  });

  it('joins two with and', () => {
    expect(listed(['a date', 'a genre'])).toBe('a date and a genre');
  });

  it('commas the rest and keeps the and for the last', () => {
    expect(listed(['a', 'b', 'c'])).toBe('a, b and c');
  });
});

describe('the tags a listener misses', () => {
  it('spells each one the way the page says it', () => {
    expect(tagName['album-artist']).toBe('album artist');
    expect(tagName['track-number']).toBe('track number');
  });

  it('has a word for every tag the server will narrow by', () => {
    expect(Object.keys(tagName).sort()).toEqual([
      'album',
      'album-artist',
      'artist',
      'artwork',
      'date',
      'genre',
      'track-number',
    ]);
  });
});

describe('a file name', () => {
  it('says the format at its end', () => {
    expect(extension('Blue Note/Sierra/01 - Dundunbanza.flac')).toBe('FLAC');
    expect(extension('01.mp3')).toBe('MP3');
  });

  it('says nothing where the name carries no end', () => {
    expect(extension('Blue Note/Sierra/cover')).toBe('');
    expect(extension('.hidden')).toBe('');
  });
});
