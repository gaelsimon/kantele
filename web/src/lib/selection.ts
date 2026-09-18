// What the detail pane is showing. One at a time, so the page stops growing downward.

import type { FolderRow } from './api';

export type Selection =
  | { kind: 'folder'; row: FolderRow }
  /// A file, and the folder it was reached under, which is the way back.
  | { kind: 'file'; path: string; under: string };

/// The last part of a path, which is what a heading shows. The whole path is the crumb above it.
export function leaf(path: string): string {
  const parts = path.split('/').filter((part) => part.length > 0);
  return parts[parts.length - 1] ?? path;
}
