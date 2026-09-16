import { describe, expect, it } from 'vitest';
import type { Setting } from './api';
import { arrange, shapeOf } from './settings';

const setting = (key: string, as_written: unknown = '', extra: Partial<Setting> = {}): Setting => ({
  key,
  label: key,
  value: String(as_written),
  as_written,
  source: { layer: 'file' },
  apply: 'immediate',
  says: 'applies at once',
  writable: true,
  ...extra,
});

const titles = (settings: Setting[]) => arrange(settings).map((block) => block.title);
const keysIn = (settings: Setting[], title: string) =>
  arrange(settings)
    .find((block) => block.title === title)
    ?.settings.map((one) => one.key);

describe('the blocks the settings are drawn in', () => {
  it('leaves out a block the server said nothing about', () => {
    expect(titles([setting('friendly_name')])).toEqual(['Network']);
  });

  it('holds the keys in the order the block names them, not the order they arrived', () => {
    const sent = [setting('http_port', 8200), setting('friendly_name', 'Kantele')];
    expect(keysIn(sent, 'Network')).toEqual(['friendly_name', 'http_port']);
  });

  it('shows a key no block names rather than dropping it', () => {
    const sent = [setting('friendly_name'), setting('scan.follow_links', 'no')];
    expect(titles(sent)).toEqual(['Network', 'Other settings']);
    expect(keysIn(sent, 'Other settings')).toEqual(['scan.follow_links']);
  });

  it('adds no block of its own where every key has a home', () => {
    expect(titles([setting('menus.axes', ['albums'])])).toEqual(['Menus']);
  });
});

describe('the control a key is edited with', () => {
  it('is the one written for it', () => {
    expect(shapeOf(setting('scan.sweep_minutes', 15))).toEqual({
      kind: 'number',
      unit: 'min',
      note: 'zero never looks on a timer',
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
  });
});
