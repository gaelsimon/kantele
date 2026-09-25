// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Library from './Library.svelte';
import type { Configuration, FolderFiles, FolderListing, FolderRow, Status } from './api';

function folder(path: string, name: string, tracks: number): FolderRow {
  return {
    path,
    name,
    folders: 0,
    tracks,
    albums: tracks > 0 ? 1 : 0,
    problems: 0,
    notes: 0,
    checks: 0,
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

  it('says the interval the timed looks run at, which a look that finds nothing leaves no trace of', async () => {
    const hours = 2 * 3_600;
    const status = {
      library: { tracks: 5, albums: 2, artists: 2, playlists: 0, untagged: 0 },
      sweep_minutes_effective: 360,
      last_pass: {
        ended: Date.now() / 1000 - hours,
        seconds: 4,
        whole_tree: true,
        outcome: 'agreed',
      },
    } as unknown as Status;
    const configuration = {
      settings: [
        { key: 'content_dir', value: '/volume1/music', as_written: '/volume1/music' },
        { key: 'scan.sweep_minutes', as_written: 15 },
      ],
    } as unknown as Configuration;
    render(Library, { props: { status, configuration, onrescanned: () => {} } });
    expect(screen.getByText(/The server looks for changes every 6 h\./)).toBeTruthy();
    expect(screen.queryByText(/Next check is due/)).toBeNull();
  });

  it('sends a first start to Settings, since there is no folder to show', async () => {
    const configuration = {
      settings: [{ key: 'content_dir', value: '', as_written: [] }],
    } as unknown as Configuration;
    const onsettings = vi.fn();
    render(Library, { props: { status: null, configuration, onrescanned: () => {}, onsettings } });
    expect(screen.queryByRole('button', { name: 'Rescan all' })).toBeNull();
    screen.getByRole('button', { name: 'Select the folder' }).click();
    expect(onsettings).toHaveBeenCalledOnce();
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
  it('keeps every column to what needs fixing once asked to', async () => {
    render(Library, {
      props: { status: null, configuration: null, onrescanned: () => {} },
    });
    await waitFor(() => expect(screen.getByText('Blue Note')).toBeTruthy());
    await fireEvent.click(screen.getByLabelText('Something to fix'));
    // The columns start again from the top under the new filter.
    await fireEvent.click(await screen.findByText('Blue Note'));
    await waitFor(() => {
      const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
      expect(asked).toContain('/api/folders?review=true');
      expect(asked).toContain('/api/folders?under=Blue+Note&review=true');
    });
  });
  it('goes back from a file to the folder it was in rather than to the whole library', async () => {
    const root = folder('', 'music', 5);
    const blue = folder('Blue Note', 'Blue Note', 3);
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.startsWith('/api/track')) return new Response('', { status: 404 });
        if (url.startsWith('/api/folders')) {
          const under = url.includes('under=') ? blue : root;
          const folders = under === root ? [blue] : [];
          const body: FolderListing = { under: under.path, here: under, folders, more: false };
          return new Response(JSON.stringify(body), { status: 200 });
        }
        const inBlue = url.includes('folder=Blue');
        const body: FolderFiles = {
          folder: inBlue ? 'Blue Note' : '',
          files: inBlue ? [{ path: 'Blue Note/01.flac', title: 'Dundunbanza', seconds: 180 }] : [],
          albums: [],
          more: false,
        };
        return new Response(JSON.stringify(body), { status: 200 });
      }),
    );
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    await fireEvent.click(await screen.findByText('Blue Note'));
    await fireEvent.click(await screen.findByText('Dundunbanza'));
    await fireEvent.click(await screen.findByText(/‹ Blue Note/));
    expect(await screen.findByRole('heading', { name: 'Blue Note' })).toBeTruthy();
  });

  it('counts a missing tag among what needs fixing, and applies it at every level', async () => {
    const coverage = { tracks: 5, date: 5, genre: 5, artwork: 5, artist: 2 };
    const status = {
      library: { tracks: 5, albums: 2, artists: 2, playlists: 0, untagged: 0, coverage },
    } as unknown as Status;
    render(Library, { props: { status, configuration: null, onrescanned: () => {} } });
    await fireEvent.click(await screen.findByText(/3 tracks with no artist/));
    expect((screen.getByLabelText('Something to fix') as HTMLInputElement).checked).toBe(true);
    await fireEvent.click(await screen.findByText('Blue Note'));
    await waitFor(() => {
      const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
      expect(asked).toContain('/api/folders?under=Blue+Note&missing=artist');
    });
    await fireEvent.click(screen.getByLabelText('Something to fix'));
    expect(screen.getByText(/3 tracks with no artist/).classList.contains('here')).toBe(false);
  });
});
