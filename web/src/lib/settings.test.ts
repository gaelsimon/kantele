import { describe, expect, it } from 'vitest';
import type { Setting } from './api';
import { ADVANCED, PLAYERS, YOUR_MUSIC, arrange, shapeOf } from './settings';

const setting = (key: string, as_written: unknown = '', extra: Partial<Setting> = {}): Setting => ({
  key,
  label: key,
  value: String(as_written),
  as_written,
  source: { layer: 'file' },
  apply: 'immediate',
  says: 'This change applies now.',
  tag: 'now',
  writable: true,
  ...extra,
});

const KEYS = [
  'content_dir',
  'friendly_name',
  'http_port',
  'state_dir',
  'capture_dir',
  'icon',
  'scan.threads',
  'scan.sweep_minutes',
  'scan.exclude',
  'scan.cover_art',
  'menus.album_threshold',
  'menus.alpha_group',
  'menus.recent',
  'menus.axes',
  'menus.sort_ignore',
  'clients',
  'log level',
];

const titles = (settings: Setting[]) => arrange(settings).map((section) => section.title);
const keysIn = (settings: Setting[], title: string) =>
  arrange(settings)
    .find((section) => section.title === title)
    ?.settings.map((one) => one.key);

describe('the three sections the settings are drawn in', () => {
  it('are always the three, in the order a first start needs them', () => {
    expect(titles([])).toEqual([YOUR_MUSIC, PLAYERS, ADVANCED]);
    expect(titles(KEYS.map((key) => setting(key)))).toEqual([YOUR_MUSIC, PLAYERS, ADVANCED]);
  });

  it('give every key the server has exactly one home', () => {
    const placed = arrange(KEYS.map((key) => setting(key))).flatMap((section) =>
      section.settings.map((one) => one.key),
    );
    expect([...placed].sort()).toEqual([...KEYS].sort());
  });

  it('hold the keys in the order the section names them, not the order they arrived', () => {
    const sent = [setting('menus.recent', 300), setting('friendly_name', 'Kantele')];
    expect(keysIn(sent, PLAYERS)).toEqual(['friendly_name', 'menus.recent']);
  });

  it('put a key no section names in Advanced options rather than dropping it', () => {
    const sent = [setting('http_port', 8200), setting('scan.follow_links', 'no')];
    expect(keysIn(sent, ADVANCED)).toEqual(['http_port', 'scan.follow_links']);
  });

  it('tag each field only where the section mixes costs', () => {
    const each = Object.fromEntries(arrange([]).map((section) => [section.title, section.each]));
    expect(each).toEqual({ [YOUR_MUSIC]: false, [PLAYERS]: false, [ADVANCED]: true });
  });
});

describe('the control a key is edited with', () => {
  it('is the one written for it, with the words that make the value a sentence', () => {
    expect(shapeOf(setting('menus.alpha_group', 100))).toMatchObject({
      kind: 'number',
      before: 'on lists of',
      after: 'or more',
    });
  });

  it('follows the value where nobody wrote one', () => {
    expect(shapeOf(setting('scan.follow_links', 'no')).kind).toBe('text');
    expect(shapeOf(setting('scan.depth', 4)).kind).toBe('number');
    expect(shapeOf(setting('scan.also', ['one', 'two'])).kind).toBe('words');
  });

  it('is a choice where the server sent the choices', () => {
    const offered = setting('scan.order', 'name', {
      choices: [
        { value: 'name', label: 'Name' },
        { value: 'date', label: 'Date' },
      ],
    });
    expect(shapeOf(offered).kind).toBe('choice');
  });

  it('only shows a value it has no control for', () => {
    expect(shapeOf(setting('scan.strict', true)).kind).toBe('read');
    expect(shapeOf(setting('state_dir', '/state')).kind).toBe('read');
  });
});
