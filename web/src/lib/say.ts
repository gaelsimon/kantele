// Numbers and times in the words the page uses. One place, so the tabs agree.

import type { Missing } from './api';

export const count = (n: number) => n.toLocaleString();

export const tagName: Record<Missing, string> = {
  artist: 'artist',
  album: 'album',
  'album-artist': 'album artist',
  date: 'date',
  genre: 'genre',
  'track-number': 'track number',
  artwork: 'cover art',
};

/// The noun alone, singular where there is one of whatever it counts.
export function noun(n: number, singular: string, plural = `${singular}s`): string {
  return n === 1 ? singular : plural;
}

/// A count and its noun: "1 album", "21 albums".
export function many(n: number, singular: string, plural = `${singular}s`): string {
  return `${count(n)} ${noun(n, singular, plural)}`;
}

/// What a file calls itself at the end of its name, which is the format an owner speaks of.
export function extension(path: string): string {
  const name = path.slice(path.lastIndexOf('/') + 1);
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toUpperCase() : '';
}

/// Rounded to the two largest units that are not zero.
export function uptime(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  if (days > 0) return `${days} d ${hours} h`;
  if (hours > 0) return `${hours} h ${minutes} min`;
  if (minutes > 0) return `${minutes} min`;
  return `${Math.floor(seconds)} s`;
}

/// How long a pass took, which is seconds until it is not.
export function took(seconds: number): string {
  if (seconds < 1) return 'under a second';
  if (seconds < 90) return `${seconds < 10 ? seconds.toFixed(1) : Math.round(seconds)} s`;
  const minutes = Math.round(seconds / 60);
  return minutes < 90 ? `${minutes} min` : `${Math.round(minutes / 60)} h`;
}

/// How long ago, against the reader's own clock.
export function ago(epochSeconds: number, now = Date.now() / 1000): string {
  const since = Math.max(0, Math.round(now - epochSeconds));
  if (since < 60) return 'just now';
  if (since < 3_600) return `${Math.floor(since / 60)} min ago`;
  if (since < 86_400) return `${Math.floor(since / 3_600)} h ago`;
  return `${Math.floor(since / 86_400)} d ago`;
}

export function clock(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
}

/// A list in prose: "a, b and c".
export function listed(parts: string[]): string {
  if (parts.length <= 1) return parts[0] ?? '';
  return `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`;
}
