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
  missing: number;
  changed: boolean;
  says: string;
  issues: Issue[];
};

/// A tag the tree can be narrowed to the folders that lack it.
export type Missing =
  | 'artist'
  | 'album'
  | 'album-artist'
  | 'date'
  | 'genre'
  | 'track-number'
  | 'artwork';

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
  writable: boolean;
  choices?: Choice[];
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

export const getStatus = () => ask<Status>('/api/status');
export const getProgress = () => ask<Progress>('/api/progress');
export const getConfiguration = () => ask<Configuration>('/api/config');

export function getFolders(
  under: string,
  search: string,
  changed: boolean,
  missing: Missing | null = null,
) {
  const asked = new URLSearchParams();
  if (under) asked.set('under', under);
  if (search) asked.set('q', search);
  if (changed) asked.set('changed', 'true');
  if (missing) asked.set('missing', missing);
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
