// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Library from './Library.svelte';
import type { FolderFiles, FolderListing, FolderRow } from './api';

function folder(path: string, name: string, tracks: number): FolderRow {
  return {
    path,
    name,
    folders: 0,
    tracks,
    albums: tracks > 0 ? 1 : 0,
    problems: 0,
    notes: 0,
    missing: 0,
    changed: false,
    says: `${tracks} tracks`,
    issues: [],
  };
}

/// The server, answered from memory: the root holds two folders, and neither holds files.
function answering() {
  const root = folder('', 'music', 5);
  const listing: FolderListing = {
    under: '',
    here: root,
    folders: [folder('Blue Note', 'Blue Note', 3), folder('Kremerata', 'Kremerata', 2)],
    more: false,
  };
  const files: FolderFiles = { folder: '', files: [], albums: [], more: false };
  return vi.fn(async (url: string) => {
    const body = url.startsWith('/api/folders') ? listing : files;
    return new Response(JSON.stringify(body), { status: 200 });
  });
}

describe('the library page', () => {
  beforeEach(() => vi.stubGlobal('fetch', answering()));
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('lists the folders the server answers with, one row each', async () => {
    render(Library, {
      props: { status: null, configuration: null, onrescanned: () => {} },
    });
    await waitFor(() => expect(screen.getByText('Blue Note')).toBeTruthy());
    expect(screen.getByText('Kremerata')).toBeTruthy();
  });

  it('offers the search box and the rescan of everything', async () => {
    render(Library, {
      props: { status: null, configuration: null, onrescanned: () => {} },
    });
    expect(screen.getByPlaceholderText('Search folders')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Rescan all' })).toBeTruthy();
  });

  it('asks the server for the top of the tree first', async () => {
    render(Library, {
      props: { status: null, configuration: null, onrescanned: () => {} },
    });
    await waitFor(() => {
      const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
      expect(asked.some((url: string) => url === '/api/folders')).toBe(true);
    });
  });
});
