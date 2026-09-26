// Where the page is, as an address: the tab, the folder or file shown, and the checks ticked.

import type { Flag } from './api';

/// Where the server serves the page. Everything under it answers with the same page.
export const BASE = '/config';

export type Tab = 'library' | 'settings';

export type Route = {
  /// Nothing where the address names no tab, which leaves the choice to the page.
  tab: Tab | null;
  /// A path relative to the music folder, empty for the library itself.
  path: string;
  only: Flag[];
};

export function parse(pathname: string, search: string): Route {
  const rest = pathname.startsWith(BASE) ? pathname.slice(BASE.length) : '';
  const [, first = '', ...parts] = rest.split('/');
  const tab = first === 'library' || first === 'settings' ? first : null;
  const path =
    tab === 'library'
      ? parts
          .filter(Boolean)
          .map((part) => decodeURIComponent(part))
          .join('/')
      : '';
  const only = (new URLSearchParams(search).get('only') ?? '').split(',').filter(Boolean);
  return { tab, path, only: tab === 'library' ? only : [] };
}

export function href(route: { tab: Tab; path?: string; only?: Flag[] }): string {
  const path = (route.path ?? '').split('/').filter(Boolean).map(encodeURIComponent);
  const at = [BASE, route.tab, ...(route.tab === 'library' ? path : [])].join('/');
  const only = route.tab === 'library' && route.only?.length ? `?only=${route.only.join(',')}` : '';
  return `${at}${only}`;
}
