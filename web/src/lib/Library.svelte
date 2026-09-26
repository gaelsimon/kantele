<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import {
    getFiles,
    getFlags,
    getFolders,
    rescan,
    type FileRow,
    type Configuration,
    type Declared,
    type FolderAlbum,
    type Flag,
    type FolderRow,
    type Status,
  } from './api';
  import { ago, count, extension, many, noun, took } from './say';
  import Columns from './Columns.svelte';
  import { above, type Entry, type Level } from './columns';
  import Detail from './Detail.svelte';
  import FilterMenu from './FilterMenu.svelte';
  import { href, parse } from './route';
  import { leaf, type Selection } from './selection';

  /// What a column of this listing carries back: the row of a folder, or the file itself.
  type Held = { kind: 'folder'; row: FolderRow } | { kind: 'file'; file: FileRow };

  let {
    status,
    configuration,
    onrescanned,
    onsettings = () => {},
  }: {
    status: Status | null;
    configuration: Configuration | null;
    onrescanned: () => void;
    onsettings?: () => void;
  } = $props();

  let search = $state('');
  let changedFirst = $state(false);
  /// Where the address says to open, read once: from then on the page writes the address.
  const arrived = parse(location.pathname, location.search);
  let ticked = $state<Flag[]>(arrived.only);
  let asking = $state(false);
  let said = $state('');
  /// What the detail pane is showing. One at a time.
  let selection = $state<Selection | null>(null);
  /// What the columns open down to: the address's path, then each one picked.
  let landing = $state(arrived.path);

  /// A click is a step Back can undo; an arrow key or a check ticked only corrects the address.
  function write(how: 'push' | 'replace') {
    const at = href({ tab: 'library', path: landing, only: ticked });
    if (at === `${location.pathname}${location.search}`) return;
    if (how === 'push') history.pushState(null, '', at);
    else history.replaceState(history.state, '', at);
  }

  $effect(() => {
    void ticked;
    untrack(() => write('replace'));
  });

  $effect(() => {
    const moved = () => {
      const now = parse(location.pathname, location.search);
      if (now.tab !== 'library') return;
      if (now.only.join() !== ticked.join()) ticked = now.only;
      if (!now.path) selection = null;
      landing = now.path;
    };
    window.addEventListener('popstate', moved);
    return () => window.removeEventListener('popstate', moved);
  });
  /// What the files of each folder read belong to, which the pane shows for the one selected.
  let albums = $state<Record<string, FolderAlbum[]>>({});
  /// The library's own row, which the pane shows while nothing else is picked.
  let root = $state<FolderRow | null>(null);
  /// Every folder row a column has read, so going back from a file lands on its folder.
  const known: Record<string, FolderRow> = {};

  const folder = $derived(
    configuration?.settings.find((setting) => setting.key === 'content_dir')?.value ?? '',
  );
  const folderless = $derived(configuration !== null && !folder);

  /// What the pane is showing: the folder picked, or the library itself.
  const shown = $derived(selection?.kind === 'folder' ? selection.row : selection ? null : root);
  const held = $derived(shown ? (albums[shown.path] ?? null) : null);

  /// The box asks again on every keystroke, and the columns are filled from the word that settled.
  let applied = $state('');
  $effect(() => {
    const wanted = search.trim();
    const settle = setTimeout(() => (applied = wanted), wanted ? 200 : 0);
    return () => clearTimeout(settle);
  });

  /// What fills a column: what is under a folder, and what is in it. A new one of these is what
  /// tells the columns that a filter changed.
  const fill = $derived.by(() => {
    const wanted = applied;
    const only = changedFirst;
    // Every level keeps to what is ticked, so the columns lead down to it and nowhere else.
    const flags = ticked;
    return async (path: string, first: boolean): Promise<Level<Held>> => {
      const [folders, files] = await Promise.all([
        getFolders(path, first ? wanted : '', first && only, flags),
        getFiles(path, flags),
      ]);
      albums = { ...albums, [path]: files.albums };
      if (first) root = folders.here;
      known[path] = folders.here;
      for (const row of folders.folders) known[row.path] = row;
      return {
        entries: [
          ...folders.folders.map((row) => ({
            path: row.path,
            // A search answers from the whole tree, so a match says where it is.
            name: first && wanted ? row.path : row.name,
            folder: true,
            of: { kind: 'folder' as const, row },
          })),
          ...files.files.map((file) => ({
            path: file.path,
            name: file.title || leaf(file.path),
            folder: false,
            icon: 'note' as const,
            of: { kind: 'file' as const, file },
          })),
        ],
        more: folders.more || files.more,
      };
    };
  });

  /// Back from a file is to the folder it was reached under, or to the library at the top.
  function back() {
    const up = selection?.kind === 'file' ? known[selection.under] : undefined;
    selection = up?.path ? { kind: 'folder', row: up } : null;
    landing = up?.path ?? '';
    write('push');
  }

  function picked(entry: Entry<Held>, how: 'click' | 'key' | 'land') {
    selection =
      entry.of.kind === 'folder'
        ? { kind: 'folder', row: entry.of.row }
        : { kind: 'file', path: entry.path, under: above(entry.path) };
    landing = entry.path;
    if (how !== 'land') write(how === 'click' ? 'push' : 'replace');
  }

  function opened(path: string, under: string) {
    selection = { kind: 'file', path, under };
    landing = path;
    write('push');
  }

  const counts = $derived([
    { of: 'album', n: status?.library.albums ?? 0 },
    { of: 'artist', n: status?.library.artists ?? 0 },
    { of: 'track', n: status?.library.tracks ?? 0 },
    { of: 'playlist', n: status?.library.playlists ?? 0 },
  ]);

  /// What the last pass did and what became of it, in one line.
  const lastCheck = $derived.by(() => {
    const pass = status?.last_pass;
    if (!pass) return 'No check since this server started.';
    const what = pass.whole_tree ? 'Whole library' : 'The folders that changed';
    const ran = `${what}, ${ago(pass.ended)}, in ${took(pass.seconds)}.`;
    const became =
      pass.outcome === 'agreed'
        ? 'Nothing changed.'
        : pass.outcome === 'published'
          ? 'The menus were updated.'
          : (pass.why ?? '');
    return `${ran} ${became}`.trim();
  });

  /// How often the server looks of its own accord. A look that finds nothing records no check, so
  /// when the next one falls cannot be told from the last.
  const nextCheck = $derived.by(() => {
    if (!status) return '';
    const minutes = status.sweep_minutes_effective;
    return minutes
      ? `The server looks for changes every ${took(minutes * 60)}.`
      : 'The server looks only when something changes.';
  });

  /// The checks the server offers. A new one appears here without the page knowing its name.
  let declared = $state<Declared[]>([]);
  onMount(() => {
    getFlags()
      .then((flags) => (declared = flags))
      .catch(() => {});
  });

  /// Where a fix is made, which is how the checks are grouped.
  const groups = [
    { id: 'files', name: 'Files', glyph: '▲', warn: true },
    { id: 'tags', name: 'Tags', glyph: '✎', warn: false },
  ];
  const every = $derived(declared.map((one) => one.flag));

  /// Kept in the order of the boxes, so the same boxes ask the server the same question.
  function tick(flags: Flag[], on: boolean) {
    ticked = every.filter((one) => (flags.includes(one) ? on : ticked.includes(one)));
  }

  function holding(row: FolderRow): string {
    const links = row.links > 0 ? `, ${many(row.links, 'playlist link')}` : '';
    return `${many(row.found, 'file')}${links}`;
  }

  async function askForAPass(which?: string) {
    asking = true;
    try {
      said = (await rescan(which)).says;
    } catch (error) {
      said = error instanceof Error ? error.message : String(error);
    } finally {
      asking = false;
      onrescanned();
    }
  }
</script>

{#snippet before(entry: Entry<Held>)}
  {#if entry.of.kind === 'file' && entry.of.file.number}
    <span class="dim num">{entry.of.file.number}</span>
  {/if}
{/snippet}

{#snippet beside(entry: Entry<Held>)}
  {#if entry.of.kind === 'file'}
    <span class="dim format">{extension(entry.path)}</span>
  {/if}
{/snippet}

{#snippet mark(entry: Entry<Held>)}
  {#if entry.of.kind === 'folder'}
    {@const row = entry.of.row}
    {#if changedFirst && row.changed}<span class="fresh">read again</span>{/if}
    {#if ticked.length > 0 && row.found + row.links > 0}
      <span class="dim num" title={holding(row)}>{count(row.found + row.links)}</span>
    {/if}
  {/if}
{/snippet}

{#snippet bounded(_at: number)}
  The list does not show all of them. Use the search box to find the others.
{/snippet}

<!-- A column lists the folders and the tracks, so an empty one is not an empty folder: what is
     left in it is what the library does not index. -->
{#snippet nothing(at: number)}
  {#if at === 0 && applied}
    No folder in this library has that name.
  {:else if ticked.length > 0}
    Nothing to fix here.
  {:else if at > 0}
    This folder has no subfolder and no track.
  {:else if changedFirst}
    The last check found no change.
  {:else}
    This library is empty.
  {/if}
{/snippet}

{#if folderless}
  <section class="card empty">
    <div class="cap">No music folder</div>
    <div class="sentence">Your players see this server, but it has no music to show them yet.</div>
    <div class="asking">
      <button class="button strong" onclick={onsettings}>Select the folder</button>
      <span class="dim aside">in Settings, then click Save</span>
    </div>
  </section>
{:else}
  <div class="boards">
    <section class="card serving">
      <div class="cap">On your players</div>
      <div class="counts">
        {#each counts as one (one.of)}
          <div class="one">
            <div class="big num">{count(one.n)}</div>
            <div class="dim">{noun(one.n, one.of)}</div>
          </div>
        {/each}
      </div>
    </section>

    <section class="card check">
      <div class="cap">Last check</div>
      <div class="sentence">{lastCheck} {nextCheck}</div>
      <div class="asking">
        <button class="button" disabled={asking} onclick={() => askForAPass()}>Rescan all</button>
        <span class="dim aside">{said || 'your players keep working meanwhile'}</span>
      </div>
    </section>
  </div>

  <section class="card tree">
    <div class="head">
      <div class="about">
        <div class="mono where">{folder}</div>
        <div class="dim aside">Select a folder or a file to see how it shows on your players.</div>
      </div>
    </div>
    <div class="bar">
      <input class="search" type="search" placeholder="Search folders" bind:value={search} />
      {#each groups as group (group.id)}
        {@const boxes = declared.filter((one) => one.group === group.id)}
        {#if boxes.length > 0}
          <FilterMenu
            name={group.name}
            glyph={group.glyph}
            warn={group.warn}
            {boxes}
            {ticked}
            ontick={tick}
          />
        {/if}
      {/each}
      <button
        class="filter-chip"
        class:on={changedFirst}
        aria-pressed={changedFirst}
        onclick={() => (changedFirst = !changedFirst)}
      >
        Changed at the last check
      </button>
      {#if ticked.length > 0 && root}
        <span class="found num">
          {holding(root)}
          <button class="quiet" onclick={() => (ticked = [])}>Clear</button>
        </span>
      {/if}
    </div>
    <div class="panes">
      <Columns
        framed={false}
        load={fill}
        {landing}
        reload={status?.system_update_id ?? 0}
        selected={selection?.kind === 'folder' ? selection.row.path : (selection?.path ?? null)}
        onpick={picked}
        {before}
        {beside}
        {mark}
        {nothing}
        {bounded}
      />
      <Detail
        row={shown}
        file={selection?.kind === 'file' ? { path: selection.path, under: selection.under } : null}
        albums={held}
        busy={asking}
        onopen={opened}
        onback={back}
        onrescan={askForAPass}
      />
    </div>

  </section>
{/if}

<style>
  .boards {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr);
    gap: 24px;
    margin-bottom: 24px;
  }

  .serving,
  .check,
  .empty {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .counts {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 16px;
  }

  .one {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
  }

  .big {
    font-size: 22px;
    line-height: 1.1;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .sentence {
    font-size: 16px;
    line-height: 1.45;
  }

  .asking {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-top: auto;
    min-width: 0;
  }

  .aside {
    font-size: 13px;
    min-width: 0;
  }

  .tree {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  /* The table and the pane are two kinds of thing. Folding one inside the other is what made the
     rows grow taller than the screen. */
  /* One box, two panes: the columns and the inspector, parted by a hairline. The inspector is a
     sidebar of its own width, so the columns are what grows when the window does. */
  .panes {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(320px, 440px);
    align-items: stretch;
    border: 1px solid var(--hairline);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    flex-wrap: wrap;
  }

  .about {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .where {
    font-size: 16px;
    font-weight: 600;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .search {
    width: 260px;
    height: 36px;
    border: 1px solid var(--faint);
    background: var(--surface);
    padding: 0 12px;
    font-size: 14px;
  }

  /* One line under the folder's name: the search, the filters, and what they hold at the far end. */
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .found {
    margin-left: auto;
    display: inline-flex;
    gap: 10px;
    align-items: baseline;
    font-size: 13px;
    white-space: nowrap;
  }

  .fresh {
    color: var(--link);
    font-size: 11px;
    white-space: nowrap;
  }

  .format {
    font-size: 11px;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }

  @media (max-width: 720px) {
    .boards {
      grid-template-columns: minmax(0, 1fr);
      gap: 16px;
      margin-bottom: 16px;
    }

    .panes {
      grid-template-columns: minmax(0, 1fr);
    }

    .counts {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 14px;
    }

    .big {
      font-size: 26px;
    }

    .asking {
      flex-direction: column;
      align-items: stretch;
    }

    .asking .button {
      height: 44px;
      justify-content: center;
    }

    .search {
      flex: 1 1 100%;
      width: auto;
      min-width: 0;
      height: 44px;
    }

    .bar :global(.filter-chip) {
      height: 44px;
    }

    .found {
      margin-left: 0;
      flex-basis: 100%;
    }
  }
</style>
