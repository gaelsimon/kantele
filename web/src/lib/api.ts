// What the server answers, and the asking. The shapes mirror src/api/.

export type Coverage = {
  tracks: number;
  artist: number;
  album: number;
  album_artist: number;
  date: number;
  genre: number;
  track_number: number;
  disc_number: number;
  composer: number;
  recording_mbid: number;
  artwork: number;
};

export type Refusal = { subject: string; detail?: string };

/// One thing wrong with a folder. `opens` says whether asking for its files will name any.
export type Issue = {
  cause: string;
  label: string;
  count: number;
  subject: string;
  problem: boolean;
  opens: boolean;
};

export type ProblemFiles = {
  folder: string;
  cause: string;
  label: string;
  total: number;
  shown: Refusal[];
};

export type Reported = {
  cause: string;
  origin: 'pass' | 'index';
  says: string;
  total: number;
  shown: Refusal[];
};

export type LastPass = {
  ended: number;
  seconds: number;
  whole_tree: boolean;
  found: number;
  opened: number;
  reused: number;
  complete: boolean;
  published: boolean;
  outcome: 'ran' | 'published' | 'agreed' | 'kept' | 'failed';
  why?: string;
  stored: boolean;
};

export type Status = {
  name: string;
  version: string;
  udn: string;
  state: 'serving' | 'indexing';
  uptime_seconds: number;
  system_update_id: number;
  /// Minutes between the timed looks as they run, stretched where the folders are watched.
  sweep_minutes_effective?: number;
  library: {
    tracks: number;
    albums: number;
    artists: number;
    playlists: number;
    untagged: number;
    coverage: Coverage;
  };
  store: { path?: string; open: boolean };
  memory_bytes?: number;
  last_pass: LastPass | null;
  failing?: { since: number; why: string };
  refused: { total: number; walked: boolean; causes: Reported[] };
};

export type Progress = {
  phase: 'idle' | 'walking' | 'reading' | 'building';
  folders: number;
  found: number;
  read: number;
  began?: number;
};

export type FolderRow = {
  path: string;
  name: string;
  folders: number;
  tracks: number;
  albums: number;
  problems: number;
  notes: number;
  /// What the checks found in the albums below it.
  checks: number;
  /// Files below it holding one of the flags asked for, each counted once.
  found: number;
  /// Playlist links below it that a flag asked for.
  links: number;
  changed: boolean;
  artwork?: string;
  says: string;
  issues: Issue[];
};

/// What the tree can be narrowed to, one box each.
/// A check's name as `only=` takes it. The server declares them, so the page knows none by heart.
export type Flag = string;

/// One check the tree can be narrowed to, as the server declares it.
export type Declared = {
  flag: Flag;
  /// Where the fix is made: `files` in a file manager or a playlist, `tags` in a tagger.
  group: string;
  label: string;
  says: string;
  /// Whether a line parts it from the checks above it.
  apart: boolean;
};

export type FolderListing = {
  under: string;
  here: FolderRow;
  folders: FolderRow[];
  more: boolean;
};

export type Layer = 'file' | 'environment' | 'command-line' | 'default';

export type Apply = 'immediate' | 'next-pass' | 'reread' | 'restart' | 'never';

export type Choice = { value: string; label: string };

export type Setting = {
  key: string;
  label: string;
  value: string;
  as_written: unknown;
  source: { layer: Layer; path?: string; variable?: string };
  apply: Apply;
  /// The mode in words, sent with it so this page keeps no copy of the sentences.
  says: string;
  /// The same mode as a badge wears it, which is a label and not a sentence.
  tag: string;
  writable: boolean;
  choices?: Choice[];
  /// What the server does with it that the value does not say.
  note?: string;
};

export type Written = {
  written: string[];
  apply: Apply;
  needs_restart: boolean;
  says: string;
};

export type ShareEntry = { path: string; name: string; folder: boolean };

export type Shares = {
  under: string;
  above: string | null;
  entries: ShareEntry[];
  more: boolean;
};

export type Configuration = { file?: string; settings: Setting[] };

export type Rescan = { started: boolean; says: string };

/// The end of the server's own log, oldest line first.
export type LogTail = { path: string; lines: string[] };

export async function getLog(lines = 200): Promise<LogTail> {
  return ask(`/api/log?lines=${lines}`);
}

/// The body, or the server's own words as the error, which are written to be read.
async function ask<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: { Accept: 'application/json', ...(init?.headers ?? {}) },
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(text.trim() || `${response.status} ${response.statusText}`);
  }
  return JSON.parse(text) as T;
}

/// One entry of a menu level. `chosen` marks a menu the owner asked for, as against one the
/// library gives.
export type MenuEntry = { at: string; title: string; children: number; chosen: boolean };

export type Menu = {
  at: string;
  kind: 'root' | 'axes' | 'values' | 'letters' | 'tracks';
  entries: MenuEntry[];
  tracks: number;
};

/// One place a track's tags put it in the menus.
export type Place = { facet: string; axis: string; value: string; at: string; tracks: number };

/// One row of a file's tag panel. No `value` means the file carries no such tag.
export type TrackTag = { label: string; value?: string };

/// One file of a folder, as the pane lists it before anybody opens it.
export type FileRow = {
  path: string;
  title: string;
  number?: number;
  artwork?: string;
  seconds: number;
};

/// One album the files of a folder belong to. `tracks` short of `of` is an album this folder holds
/// only a part of, which is the two-disc case.
export type FolderAlbum = {
  id: string;
  title: string;
  credit?: string;
  date?: string;
  artwork?: string;
  /// The folder the cover sits in, sent only where that is not the folder asked about.
  cover_in?: string;
  tracks: number;
  of: number;
  folders: string[];
  says?: string;
  /// What a listener would want fixed in its tags.
  checks: { says: string }[];
};

export type FolderFiles = {
  folder: string;
  files: FileRow[];
  albums: FolderAlbum[];
  more: boolean;
};

export type TrackDetail = {
  path: string;
  title: string;
  tags: TrackTag[];
  places: Place[];
  album?: { title: string; at: string; keyed_on_path: boolean };
  /// What to ask `/art/` for, where the file has a cover.
  artwork?: string;
  format: string;
  bytes: number;
  seconds: number;
  /// The other files holding the same recording.
  copies?: string[];
  /// A sentence per name this file writes as fewer files do.
  spellings?: string[];
};

export const getStatus = () => ask<Status>('/api/status');
export const getProgress = () => ask<Progress>('/api/progress');
export const getConfiguration = () => ask<Configuration>('/api/config');
export function getFiles(folder: string, only: Flag[] = []) {
  const asked = new URLSearchParams();
  if (folder) asked.set('folder', folder);
  if (only.length > 0) asked.set('only', only.join(','));
  const query = asked.toString();
  return ask<FolderFiles>(`/api/files${query ? `?${query}` : ''}`);
}

export const getFlags = () => ask<Declared[]>('/api/flags');

export const getTrack = (path: string) =>
  ask<TrackDetail>(`/api/track?path=${encodeURIComponent(path)}`);

export const getMenu = (at = '') =>
  ask<Menu>(`/api/menu${at ? `?at=${encodeURIComponent(at)}` : ''}`);

export function getFolders(under: string, search: string, changed: boolean, only: Flag[] = []) {
  const asked = new URLSearchParams();
  if (under) asked.set('under', under);
  if (search) asked.set('q', search);
  if (changed) asked.set('changed', 'true');
  if (only.length > 0) asked.set('only', only.join(','));
  const query = asked.toString();
  return ask<FolderListing>(`/api/folders${query ? `?${query}` : ''}`);
}

export function getProblems(folder: string, cause: string) {
  const asked = new URLSearchParams({ cause });
  if (folder) asked.set('folder', folder);
  return ask<ProblemFiles>(`/api/problems?${asked}`);
}

export function getShares(under: string, images: boolean) {
  const asked = new URLSearchParams();
  if (under) asked.set('under', under);
  if (images) asked.set('images', 'true');
  const query = asked.toString();
  return ask<Shares>(`/api/shares${query ? `?${query}` : ''}`);
}

export function writeConfiguration(changes: Record<string, unknown>) {
  return ask<Written>('/api/config', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(changes),
  });
}

export function rescan(folder?: string) {
  const where = folder ? `?folder=${encodeURIComponent(folder)}` : '';
  return ask<Rescan>(`/api/rescan${where}`, { method: 'POST' });
}
