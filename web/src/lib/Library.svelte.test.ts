// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Library from './Library.svelte';
import type { Configuration, Declared, FolderFiles, FolderListing, FolderRow, Status } from './api';

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
    found: 0,
    links: 0,
    changed: false,
    says: `${tracks} tracks`,
    issues: [],
  };
}

/// What the server offers to narrow by, cut down to what the tests click.
const catalogue: Declared[] = [
  { flag: 'unserved', group: 'files', label: 'Files not served', says: 'Unreadable files', apart: false },
  { flag: 'duplicate-tracks', group: 'files', label: 'Duplicate tracks', says: 'One recording twice', apart: false },
  { flag: 'no-artist', group: 'tags', label: 'No artist', says: 'Tracks with no artist tag', apart: false },
  { flag: 'no-date', group: 'tags', label: 'No date', says: 'Missing from the Date menu', apart: true },
];

const flagsOr = (url: string, body: unknown) =>
  new Response(JSON.stringify(url.startsWith('/api/flags') ? catalogue : body), { status: 200 });

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
  return vi.fn(async (url: string) => flagsOr(url, url.startsWith('/api/folders') ? listing : files));
}

describe('the library page', () => {
  beforeEach(() => vi.stubGlobal('fetch', answering()));
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    history.replaceState(null, '', '/');
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
  it('keeps every column to what a group asks for once All is ticked', async () => {
    render(Library, {
      props: { status: null, configuration: null, onrescanned: () => {} },
    });
    await fireEvent.click(await screen.findByRole('button', { name: /Files/ }));
    await fireEvent.click(screen.getByRole('button', { name: 'All' }));
    // The columns start again from the top under the new filter.
    await fireEvent.click(await screen.findByText('Blue Note'));
    const files = 'only=unserved%2Cduplicate-tracks';
    await waitFor(() => {
      const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
      expect(asked).toContain(`/api/folders?${files}`);
      expect(asked).toContain(`/api/folders?under=Blue+Note&${files}`);
    });
  });
  it('goes back from a file to the folder it was in rather than to the whole library', async () => {
    const root = folder('', 'music', 5);
    const blue = folder('Blue Note', 'Blue Note', 3);
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.startsWith('/api/track')) return new Response('', { status: 404 });
        if (url.startsWith('/api/flags')) return flagsOr(url, null);
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

  it('ticks one box at every level, and its chip says what it narrows to', async () => {
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    await fireEvent.click(await screen.findByRole('button', { name: /Tags/ }));
    await fireEvent.click(screen.getByLabelText('No artist'));
    await fireEvent.click(await screen.findByText('Blue Note'));
    await waitFor(() => {
      const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
      expect(asked).toContain('/api/folders?under=Blue+Note&only=no-artist');
    });
    expect(screen.getByRole('button', { name: /Tags: No artist/ })).toBeTruthy();
    expect(screen.queryByLabelText('No date'), 'a click elsewhere closes the menu').toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: 'Clear Tags' }));
    expect(screen.getByRole('button', { name: 'Tags' }), 'and the chip names its group alone').toBeTruthy();
  });

  it('draws a check the server declares that the page has never heard of', async () => {
    catalogue.push({ flag: 'no-composer', group: 'tags', label: 'No composer', says: 'Tracks with no composer', apart: false });
    try {
      render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
      await fireEvent.click(await screen.findByRole('button', { name: /Tags/ }));
      await fireEvent.click(screen.getByLabelText('No composer'));
      await waitFor(() => {
        const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
        expect(asked).toContain('/api/folders?only=no-composer');
      });
    } finally {
      catalogue.pop();
    }
  });

  it('counts what the boxes hold only once one is ticked', async () => {
    const root = { ...folder('', 'music', 5), found: 9, links: 2 };
    const blue = { ...folder('Blue Note', 'Blue Note', 3), found: 4 };
    const listing: FolderListing = { under: '', here: root, folders: [blue], more: false };
    const files: FolderFiles = { folder: '', files: [], albums: [], more: false };
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => flagsOr(url, url.startsWith('/api/folders') ? listing : files)),
    );
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    await screen.findByText('Blue Note');
    expect(screen.queryByText('4')).toBeNull();
    expect(screen.queryByText(/9 files/)).toBeNull();

    await fireEvent.click(await screen.findByRole('button', { name: /Tags/ }));
    await fireEvent.click(screen.getByLabelText('No date'));
    expect(await screen.findByText('4')).toBeTruthy();
    expect(screen.getByText(/9 files, 2 playlist links/)).toBeTruthy();
  });

  it('opens down to the folder its address names', async () => {
    history.replaceState(null, '', '/config/library/Blue%20Note');
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    expect(await screen.findByRole('heading', { name: 'Blue Note' })).toBeTruthy();
    const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
    expect(asked).toContain('/api/folders?under=Blue+Note');
    expect(location.pathname, 'and leaves the address as it was').toBe('/config/library/Blue%20Note');
  });

  it('ticks the checks its address names', async () => {
    history.replaceState(null, '', '/config/library?only=no-artist');
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    expect(await screen.findByRole('button', { name: /Tags: No artist/ })).toBeTruthy();
    const asked = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls.map((call) => call[0]);
    expect(asked).toContain('/api/folders?only=no-artist');
  });

  it('writes the folder shown and the checks ticked into its address', async () => {
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    await fireEvent.click(await screen.findByText('Blue Note'));
    await waitFor(() => expect(location.pathname).toBe('/config/library/Blue%20Note'));
    await fireEvent.click(screen.getByRole('button', { name: /Tags/ }));
    await fireEvent.click(screen.getByLabelText('No artist'));
    await waitFor(() => expect(location.search).toBe('?only=no-artist'));
  });

  it('goes back to the folder picked before, and then to the whole library', async () => {
    render(Library, { props: { status: null, configuration: null, onrescanned: () => {} } });
    await fireEvent.click(await screen.findByText('Blue Note'));
    await fireEvent.click(await screen.findByText('Kremerata'));
    expect(await screen.findByRole('heading', { name: 'Kremerata' })).toBeTruthy();

    history.back();
    expect(await screen.findByRole('heading', { name: 'Blue Note' })).toBeTruthy();
    expect(location.pathname).toBe('/config/library/Blue%20Note');

    history.back();
    expect(await screen.findByRole('heading', { name: 'The whole library' })).toBeTruthy();
  });
});
