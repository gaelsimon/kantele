// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import Detail from './Detail.svelte';
import type { FolderRow } from './api';

const folder: FolderRow = {
  path: 'Blue Note/Sierra Maestra',
  name: 'Sierra Maestra',
  folders: 0,
  tracks: 3,
  albums: 1,
  problems: 0,
  notes: 0,
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
});
