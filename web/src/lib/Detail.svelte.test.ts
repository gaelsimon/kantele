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
});
