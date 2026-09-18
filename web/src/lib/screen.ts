// What a player's screen lists for this server: the root as the wire builds it, drawn from
// the draft as it is edited and from what the library holds.

export type Row = { label: string; fixed: boolean };

/// What the library holds, which decides the entries that are not settings.
export type Holdings = { albums: number; tracks: number; untagged: number; playlists: number };

/// The name the server carries when the owner has typed none.
export const FALLBACK_NAME = 'Kantele';

const counted = (n: number, noun: string) => `${n} ${noun}${n === 1 ? '' : 's'}`;

/// The root, in the wire's order. The albums and every item lead, the chosen menus follow, and the
/// rest appear when there is something behind them. The number of albums shown directly spares a
/// selection narrowed by hand, never the root, so it says nothing here.
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
  if (holds.albums > 0) rows.push(fixed(counted(holds.albums, 'album')));
  rows.push(fixed(counted(holds.tracks, 'item')));
  // A menu with nothing behind it is not offered, which an empty library makes plain.
  if (holds.tracks > 0) {
    rows.push(
      ...axes.filter((label) => !hidden.includes(label)).map((label) => ({ label, fixed: false })),
    );
  }
  if (holds.untagged > 0) rows.push(fixed('[untagged]'));
  if (holds.playlists > 0) rows.push(fixed('Playlists'));
  if (recent > 0 && holds.tracks > 0) rows.push({ label: 'Recently added', fixed: false });
  if (holds.tracks > 0) rows.push(fixed('[folder view]'));
  return { name: name.trim() || FALLBACK_NAME, rows };
}
