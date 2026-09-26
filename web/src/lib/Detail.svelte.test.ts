// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import Detail from './Detail.svelte';
import type { FolderAlbum, FolderRow } from './api';

const folder: FolderRow = {
  path: 'Blue Note/Sierra Maestra',
  name: 'Sierra Maestra',
  folders: 0,
  tracks: 3,
  albums: 1,
  problems: 0,
  notes: 0,
  checks: 0,
  found: 0,
  links: 0,
  changed: false,
  says: '3 tracks, 1 album',
  issues: [],
};

describe('the detail pane', () => {
  afterEach(cleanup);

  it('names the folder it is showing', () => {
    render(Detail, {
      props: {
        row: folder,
        file: null,
        albums: null,
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    expect(screen.getByText('Sierra Maestra')).toBeTruthy();
  });

  it('lists the failures apart from what the tags left unsaid', () => {
    const row: FolderRow = {
      ...folder,
      issues: [
        { cause: 'unreadable', label: 'Unreadable file', count: 1, subject: 'file', problem: true, opens: true },
        { cause: 'no-artwork', label: 'No cover art', count: 2, subject: 'tracks', problem: false, opens: true },
      ],
    };
    render(Detail, {
      props: {
        row,
        file: null,
        albums: null,
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    const problems = screen.getByText('Problems').parentElement;
    const notes = screen.getByText('Notes').parentElement;
    expect(problems?.textContent).toContain('Unreadable file');
    expect(notes?.textContent).toContain('No cover art');
    expect(problems?.textContent).not.toContain('No cover art');
  });
  it('says under an album what to fix', () => {
    const album: FolderAlbum = {
      id: 'al-1',
      title: 'Harvest',
      tracks: 11,
      of: 11,
      folders: ['_FLAC/Harvest'],
      checks: [{ says: 'Track 7 of 12 is missing' }],
    };
    render(Detail, {
      props: {
        row: folder,
        file: null,
        albums: [album],
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    expect(screen.getByText(/Track 7 of 12 is missing/)).toBeTruthy();
  });
  it('says what is left to fix below a folder whose own files hold no album', () => {
    render(Detail, {
      props: {
        row: { ...folder, path: 'Neil Young', name: 'Neil Young', checks: 2 },
        file: null,
        albums: [],
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    expect(screen.getByText(/2 things to fix in the albums below/)).toBeTruthy();
    expect(screen.queryByText(/The tags show nothing unusual/)).toBeNull();
  });
  it('lists every broken link of one playlist, which all name the same playlist', async () => {
    const shown = [
      { subject: 'Sierra/set.m3u', detail: 'Sierra/gone-1.flac' },
      { subject: 'Sierra/set.m3u', detail: 'Sierra/gone-2.flac' },
    ];
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ folder: 'Sierra', cause: 'missing-entry', label: 'Broken playlist link', total: 2, shown }), { status: 200 })),
    );
    const row: FolderRow = {
      ...folder,
      issues: [{ cause: 'missing-entry', label: 'Broken playlist link', count: 2, subject: 'links', problem: true, opens: true }],
    };
    render(Detail, {
      props: {
        row,
        file: null,
        albums: null,
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    await fireEvent.click(screen.getByText(/Broken playlist link/));
    expect(await screen.findByText(/gone-1\.flac/)).toBeTruthy();
    expect(screen.getByText(/gone-2\.flac/)).toBeTruthy();
    expect(screen.queryByText(/not listed/)).toBeNull();
    vi.unstubAllGlobals();
  });

  it('says how many it counts and does not name', async () => {
    const shown = [{ subject: 'Sierra/set.m3u', detail: 'Sierra/gone-1.flac' }];
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ folder: 'Sierra', cause: 'missing-entry', label: 'Broken playlist link', total: 151, shown }), { status: 200 })),
    );
    const row: FolderRow = {
      ...folder,
      issues: [{ cause: 'missing-entry', label: 'Broken playlist link', count: 151, subject: 'links', problem: true, opens: true }],
    };
    render(Detail, {
      props: {
        row,
        file: null,
        albums: null,
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    await fireEvent.click(screen.getByText(/Broken playlist link/));
    expect(await screen.findByText('150 more are not listed.')).toBeTruthy();
    vi.unstubAllGlobals();
  });

  it('names the other copies of a file and opens one in the folder it sits in', async () => {
    const track = {
      path: 'Harvest/05.flac',
      title: 'Harvest Moon',
      tags: [],
      places: [],
      format: 'FLAC',
      bytes: 1,
      seconds: 300,
      copies: ['Selection/harvest moon.mp3'],
    };
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(track), { status: 200 })));
    const onopen = vi.fn();
    render(Detail, {
      props: {
        row: null,
        file: { path: 'Harvest/05.flac', under: 'Harvest' },
        albums: null,
        busy: false,
        onopen,
        onback: () => {},
        onrescan: () => {},
      },
    });
    await fireEvent.click(await screen.findByText('Selection/harvest moon.mp3'));
    expect(onopen).toHaveBeenCalledWith('Selection/harvest moon.mp3', 'Selection');
    vi.unstubAllGlobals();
  });

  it('says how the rest of the library spells a name this file spells otherwise', async () => {
    const said = 'Genre Drum n Bass is written Drum & Bass on 120 files and Drum and Bass on 30';
    const track = {
      path: 'Mixes/01.flac',
      title: 'Warning',
      tags: [],
      places: [],
      format: 'FLAC',
      bytes: 1,
      seconds: 300,
      spellings: [said],
    };
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(track), { status: 200 })));
    render(Detail, {
      props: {
        row: null,
        file: { path: 'Mixes/01.flac', under: 'Mixes' },
        albums: null,
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
      },
    });
    expect(await screen.findByText(said)).toBeTruthy();
    expect(screen.getByText('Spelled otherwise')).toBeTruthy();
    vi.unstubAllGlobals();
  });
});
