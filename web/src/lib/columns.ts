// The trail of open folders, and what one column of it lists. Nothing here knows what fills a
// column: the library fills one from its own routes, the folder chooser from the shares.

/// One row of a column: what it is called, whether it opens onto a column of its own, and whatever
/// the page that filled the column wants back when it is picked.
export type Entry<T> = {
  path: string;
  name: string;
  folder: boolean;
  /// What a row that is not a folder is drawn as. A track is a note, anything else a sheet.
  icon?: 'note' | 'file';
  of: T;
};

/// What one ask fills a column with.
export type Level<T> = { entries: Entry<T>[]; more: boolean };

/// The folders a trail draws a column for: the top of the listing, then each folder opened under it.
export function shown(trail: string[]): string[] {
  return ['', ...trail];
}

/// The trail after opening a folder in the column at `depth`. What was open below it closes.
export function opened(trail: string[], depth: number, path: string): string[] {
  return [...trail.slice(0, depth), path];
}

/// The folder a path sits in, empty for one at the top of the listing.
export function above(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? '' : path.slice(0, cut);
}

/// The trail that opens down to `path`, which sits under `root`, an entry of the first column.
/// The path itself is not on the trail: it is what the last column shows, selected.
export function trailUnder(root: string, path: string): string[] {
  if (!path.startsWith(`${root}/`)) return [];
  const parts = path.slice(root.length + 1).split('/').filter(Boolean);
  const trail = [root];
  for (const part of parts.slice(0, -1)) trail.push(`${trail[trail.length - 1]}/${part}`);
  return trail;
}

/// A name split as the Finder splits it: the end is kept whole and the middle gives way, because
/// what tells two folders apart is usually the last words. The cut moves to the nearest space, so
/// the end that is kept starts on a word.
export function halves(name: string, keep = 12): [string, string] {
  if (name.length <= keep + 6) return [name, ''];
  const cut = name.length - keep;
  const space = name.lastIndexOf(' ', cut + 4);
  const at = space >= cut - 4 && space < name.length - 2 ? space + 1 : cut;
  return [name.slice(0, at), name.slice(at)];
}

/// Where a key lands inside a column, which is never outside it.
export function stepped(at: number, by: number, count: number): number {
  if (count === 0) return 0;
  return Math.min(count - 1, Math.max(0, at + by));
}
