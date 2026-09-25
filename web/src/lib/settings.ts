import type { Setting } from './api';

export type Kind =
  | 'text'
  | 'number'
  | 'paths'
  | 'words'
  | 'chips'
  | 'choice'
  | 'image'
  | 'folder'
  | 'read';

/// How a key is edited, and the words beside the control that make the value read as a sentence.
export type Shape = { kind: Kind; before?: string; after?: string; note?: string };

export type Section = {
  title: string;
  /// Every field carries its own cost tag, because the section mixes costs.
  each: boolean;
  note?: string;
  settings: Setting[];
};

/// The server says what a setting is and what a change costs; this says what the control looks
/// like, which is the page's business.
const shapes: Record<string, Shape> = {
  content_dir: {
    kind: 'paths',
    note: 'Use only shared folders. Do not put one folder in another folder. The server does not write in these folders.',
  },
  'scan.exclude': {
    kind: 'words',
    note: 'The server does not scan these names or paths. Separate the entries with commas. The server does not scan @eaDir and #recycle.',
  },
  friendly_name: { kind: 'text' },
  'menus.axes': {
    kind: 'chips',
    note: 'Drag a menu, or use ←, to change the sequence. The dashed entries are menus you can add.',
  },
  'menus.recent': {
    kind: 'number',
    before: 'the last',
    after: 'files',
    note: 'If the value is 0, the server does not make a Recently added menu.',
  },
  'menus.album_threshold': {
    kind: 'number',
    before: 'up to',
    note: 'If a selection has more albums than this number, the menu divides it again. If not, the menu shows the albums.',
  },
  'menus.alpha_group': {
    kind: 'number',
    before: 'on lists of',
    after: 'or more',
    note: 'If the value is 0, the server does not make an A to Z index.',
  },
  'scan.sweep_minutes': {
    kind: 'number',
    after: 'min',
    note: 'The server reads only the folders that changed. When it can watch the folders, it waits much longer between scans. If the value is 0, the server does not scan on a timer.',
  },
  'scan.threads': { kind: 'number', note: 'A high value makes a NAS slower.' },
  'scan.cover_art': {
    kind: 'choice',
    note: 'If this image is not available, the server uses the other image.',
  },
  'menus.sort_ignore': {
    kind: 'words',
    note: "Examples: The, Les, L'. Separate the words with commas.",
  },
  http_port: { kind: 'number' },
  icon: { kind: 'image' },
  capture_dir: { kind: 'folder', note: 'The server writes each control exchange to this folder.' },
  state_dir: { kind: 'read' },
  clients: { kind: 'read' },
  'log level': { kind: 'read' },
};

export const YOUR_MUSIC = 'Your music';
export const PLAYERS = 'Menu on your players';
export const ADVANCED = 'Advanced options';

/// The three sections, in the order a first start needs them. The last takes every key the first
/// two do not name, so a server that grows a key does not lose it here in silence.
const sections: { title: string; each: boolean; note?: string; keys: string[] }[] = [
  { title: YOUR_MUSIC, each: false, keys: ['content_dir', 'scan.exclude'] },
  {
    title: PLAYERS,
    each: false,
    keys: ['friendly_name', 'menus.axes', 'menus.recent', 'menus.album_threshold', 'menus.alpha_group'],
  },
  {
    title: ADVANCED,
    each: true,
    keys: [
      'scan.sweep_minutes',
      'scan.threads',
      'scan.cover_art',
      'menus.sort_ignore',
      'http_port',
      'icon',
      'capture_dir',
      'state_dir',
      'clients',
      'log level',
    ],
    note: 'To apply a setting that needs a restart, open the DSM Package Center and restart the server. To change the device profiles and the log level, edit kantele.toml.',
  },
];

export function arrange(settings: Setting[]): Section[] {
  const named = new Set(sections.flatMap((section) => section.keys));
  const rest = settings.filter((setting) => !named.has(setting.key));
  return sections.map(({ keys, ...section }) => ({
    ...section,
    settings: [
      ...keys
        .map((key) => settings.find((setting) => setting.key === key))
        .filter((setting): setting is Setting => setting !== undefined),
      ...(section.title === ADVANCED ? rest : []),
    ],
  }));
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
