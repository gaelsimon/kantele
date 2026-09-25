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
  missing: 0,
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
        onfind: () => {},
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
        onfind: () => {},
      },
    });
    const problems = screen.getByText('Problems').parentElement;
    const notes = screen.getByText('Notes').parentElement;
    expect(problems?.textContent).toContain('Unreadable file');
    expect(notes?.textContent).toContain('No cover art');
    expect(problems?.textContent).not.toContain('No cover art');
  });
  it('says under an album what to fix, and shows its twin beside it', async () => {
    const album: FolderAlbum = {
      id: 'al-1',
      title: 'Harvest',
      tracks: 11,
      of: 11,
      folders: ['_FLAC/Harvest'],
      checks: [
        { says: 'Track 7 of 12 is missing' },
        {
          says: 'Same title and artist as the album in _ITUNES/Harvest, so your players show both',
          folder: '_ITUNES/Harvest',
        },
      ],
    };
    const onfind = vi.fn();
    render(Detail, {
      props: {
        row: folder,
        file: null,
        albums: [album],
        busy: false,
        onopen: () => {},
        onback: () => {},
        onrescan: () => {},
        onfind,
      },
    });
    expect(screen.getByText(/Track 7 of 12 is missing/)).toBeTruthy();
    await fireEvent.click(screen.getByText('Show both'));
    expect(onfind).toHaveBeenCalledWith('Harvest');
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
        onfind: () => {},
      },
    });
    expect(screen.getByText(/2 things to fix in the albums below/)).toBeTruthy();
    expect(screen.queryByText(/The tags show nothing unusual/)).toBeNull();
  });
});
