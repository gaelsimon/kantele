import type { Setting } from './api';

export type Kind = 'text' | 'number' | 'path' | 'paths' | 'words' | 'chips' | 'choice' | 'read';
export type Shape = { kind: Kind; unit?: string; note?: string; images?: boolean };

export type Block = { title: string; side: number; note?: string; settings: Setting[] };

/// How each key is edited. The server says what a setting is and what a change costs; this says
/// what the control looks like, which is the page's business.
const shapes: Record<string, Shape> = {
  content_dir: { kind: 'paths', note: 'shared folders only, and none inside another' },
  'scan.exclude': { kind: 'words', note: 'names and paths never walked, separated by commas' },
  'scan.threads': { kind: 'number', note: 'beyond a handful a NAS goes slower' },
  'scan.sweep_minutes': { kind: 'number', unit: 'min', note: 'zero never looks on a timer' },
  friendly_name: { kind: 'text' },
  http_port: { kind: 'number' },
  icon: { kind: 'path', images: true },
  state_dir: { kind: 'path' },
  capture_dir: { kind: 'path', note: 'every control exchange is written there' },
  'menus.album_threshold': { kind: 'number' },
  'menus.alpha_group': { kind: 'number', note: 'zero turns the A-Z index off' },
  'menus.recent': { kind: 'number', unit: 'files', note: 'zero offers no recently added menu' },
  'menus.axes': { kind: 'chips' },
  'menus.sort_ignore': { kind: 'words', note: "The, Les, L' and the like" },
  'scan.cover_art': {
    kind: 'choice',
    note: 'the other one is still used where this one is missing',
  },
};

/// The blocks the mockup groups by, in its order and its columns.
const blocks: { title: string; side: number; note?: string; keys: string[] }[] = [
  { title: 'Folders', side: 0, keys: ['content_dir', 'scan.exclude'] },
  { title: 'Scanning', side: 0, keys: ['scan.threads', 'scan.sweep_minutes'] },
  {
    title: 'Network',
    side: 0,
    keys: ['friendly_name', 'http_port', 'icon', 'state_dir', 'capture_dir'],
    note: "restart from DSM's Package Center for these to take effect",
  },
  {
    title: 'Menus',
    side: 1,
    keys: ['menus.album_threshold', 'menus.alpha_group', 'menus.recent', 'menus.axes'],
  },
  { title: 'Names', side: 1, keys: ['menus.sort_ignore'] },
  { title: 'Cover art', side: 1, keys: ['scan.cover_art'] },
  { title: 'In the file', side: 1, keys: ['clients', 'log level'] },
];

const REST = 'Other settings';

/// A setting no block above names is still shown, so a server that grows a key does not lose it
/// here in silence.
export function arrange(settings: Setting[]): Block[] {
  const named = new Set(blocks.flatMap((block) => block.keys));
  const grouped = blocks
    .map(({ keys, ...block }) => ({
      ...block,
      settings: keys
        .map((key) => settings.find((setting) => setting.key === key))
        .filter((setting): setting is Setting => setting !== undefined),
    }))
    .filter((block) => block.settings.length > 0);
  const rest = settings.filter((setting) => !named.has(setting.key));
  return rest.length > 0 ? [...grouped, { title: REST, side: 1, settings: rest }] : grouped;
}

export function shapeOf(setting: Setting): Shape {
  return shapes[setting.key] ?? inferred(setting);
}

/// What a key nobody wrote a shape for looks like, read from the value the server sent.
function inferred(setting: Setting): Shape {
  if (setting.choices && setting.choices.length > 0) return { kind: 'choice' };
  const written = setting.as_written;
  if (typeof written === 'number') return { kind: 'number' };
  if (typeof written === 'string') return { kind: 'text' };
  if (Array.isArray(written) && written.every((one) => typeof one === 'string')) {
    return { kind: 'words' };
  }
  return { kind: 'read' };
}
