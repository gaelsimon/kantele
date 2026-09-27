// What a player's screen lists for this server: the root as the wire builds it, drawn from
// the draft as it is edited and from what the library holds.

export type Row = { label: string; fixed: boolean };

/// What the library holds, which decides the entries that are not settings.
export type Holdings = {
  albums: number;
  tracks: number;
  untagged: number;
  playlists: number;
  compilations: number;
};

/// The name the server carries when the owner has typed none.
export const FALLBACK_NAME = 'Kantele';

/// The root, in the wire's order: the albums, the chosen menus, then the entries that appear when
/// there is something behind them, and last the two long lists. The number of albums shown
/// directly spares a selection narrowed by hand, never the root, so it says nothing here.
export function screen(
  name: string,
  axes: string[],
  recent: number,
  holds: Holdings,
  // An axis with one value narrows nothing, and the server leaves it out. Only the server knows.
  hidden: readonly string[] = [],
): { name: string; rows: Row[] } {
  const fixed = (label: string) => ({ label, fixed: true });
  const rows: Row[] = [];
  if (holds.albums > 0) rows.push(fixed('Albums'));
  // A menu with nothing behind it is not offered, which an empty library makes plain.
  if (holds.tracks > 0) {
    rows.push(
      ...axes.filter((label) => !hidden.includes(label)).map((label) => ({ label, fixed: false })),
    );
  }
  if (holds.compilations > 0) rows.push(fixed('Compilations'));
  if (recent > 0 && holds.tracks > 0) rows.push({ label: 'Recently added', fixed: false });
  if (holds.playlists > 0) rows.push(fixed('Playlists'));
  if (holds.tracks > 0) rows.push(fixed('Folders'));
  rows.push(fixed('Tracks'));
  if (holds.untagged > 0) rows.push(fixed('Untagged tracks'));
  return { name: name.trim() || FALLBACK_NAME, rows };
}
