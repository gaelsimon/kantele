<script lang="ts">
  import {
    getFiles,
    getFolders,
    rescan,
    type FileRow,
    type Configuration,
    type FolderAlbum,
    type FolderRow,
    type Missing,
    type Status,
  } from './api';
  import { ago, count, extension, many, noun, tagName, took } from './say';
  import Columns from './Columns.svelte';
  import { above, type Entry, type Level } from './columns';
  import Detail from './Detail.svelte';
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
  let lacking = $state<Missing | null>(null);
  let asking = $state(false);
  let said = $state('');
  /// What the detail pane is showing. One at a time.
  let selection = $state<Selection | null>(null);
  /// What the files of each folder read belong to, which the pane shows for the one selected.
  let albums = $state<Record<string, FolderAlbum[]>>({});
  /// The library's own row, which the pane shows while nothing else is picked.
  let root = $state<FolderRow | null>(null);

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
    const tag = lacking;
    return async (path: string, first: boolean): Promise<Level<Held>> => {
      const [folders, files] = await Promise.all([
        getFolders(path, first ? wanted : '', first && only, first ? tag : null),
        getFiles(path),
      ]);
      albums = { ...albums, [path]: files.albums };
      if (first) root = folders.here;
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

  function picked(entry: Entry<Held>) {
    selection =
      entry.of.kind === 'folder'
        ? { kind: 'folder', row: entry.of.row }
        : { kind: 'file', path: entry.path, under: above(entry.path) };
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

  /// The tags a listener will miss, counted against the whole library. Each one narrows the tree
  /// to the folders holding them, which is the only way from the number to the files.
  const missing = $derived.by(() => {
    const coverage = status?.library.coverage;
    if (!coverage || coverage.tracks === 0) return [];
    return (
      [
        { of: 'date', n: coverage.tracks - coverage.date },
        { of: 'genre', n: coverage.tracks - coverage.genre },
        { of: 'artwork', n: coverage.tracks - coverage.artwork },
        { of: 'artist', n: coverage.tracks - coverage.artist },
      ] as { of: Missing; n: number }[]
    ).filter((one) => one.n > 0);
  });

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
    {#if entry.of.row.changed}<span class="fresh">read again</span>{/if}
    {#if lacking && entry.of.row.missing > 0}
      <span class="dim num">{count(entry.of.row.missing)}</span>
    {:else if entry.of.row.problems > 0}
      <!-- The colour alone says nothing to a reader who cannot tell it apart. -->
      <span class="problem num mark"><span aria-hidden="true">▲</span>
        {count(entry.of.row.problems)}</span>
    {/if}
  {/if}
{/snippet}

{#snippet bounded(_at: number)}
  The list does not show all of them. Use the search box to find the others.
{/snippet}

<!-- A column lists the folders and the tracks, so an empty one is not an empty folder: what is
     left in it is what the library does not index. -->
{#snippet nothing(at: number)}
  {#if at > 0}
    This folder has no subfolder and no track.
  {:else if applied}
    No folder in this library has that name.
  {:else if lacking}
    Every track has {lacking === 'artwork' ? tagName[lacking] : `a ${tagName[lacking]}`}.
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
      <div class="tools">
        <input class="search" type="search" placeholder="Search folders" bind:value={search} />
        <label class="filter">
          <input type="checkbox" bind:checked={changedFirst} />
          Changed at the last check
        </label>
      </div>
    </div>
    <div class="panes">
      <Columns
        framed={false}
        load={fill}
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
        onopen={(path, under) => (selection = { kind: 'file', path, under })}
        onback={() => (selection = null)}
        onrescan={askForAPass}
      />
    </div>

    {#if missing.length > 0}
      <div class="missing">
        <span class="dim">Missing tags</span>
        {#each missing as one (one.of)}
          <button
            class="tag"
            class:here={lacking === one.of}
            title="List the folders that hold these tracks"
            onclick={() => (lacking = lacking === one.of ? null : one.of)}
          >
            {many(one.n, 'track')} with no {tagName[one.of]}
          </button>
        {/each}
        {#if lacking}
          <button class="quiet" onclick={() => (lacking = null)}>All folders</button>
        {/if}
      </div>
    {:else if status && status.library.tracks > 0}
      <div class="dim missing">All tracks have a date, a genre, an artist, and cover art.</div>
    {/if}
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
    grid-template-columns: minmax(0, 1fr) minmax(280px, 360px);
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

  .tools {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .search {
    width: 260px;
    height: 36px;
    border: 1px solid var(--faint);
    background: var(--surface);
    padding: 0 12px;
    font-size: 14px;
  }

  .filter {
    height: 36px;
    padding: 0 12px;
    border: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--ink-soft);
  }

  .missing {
    font-size: 13px;
  }

  .fresh {
    color: var(--link);
    font-size: 11px;
    white-space: nowrap;
  }

  .mark {
    font-size: 12px;
    white-space: nowrap;
  }

  .format {
    font-size: 11px;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }

  .missing {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .tag {
    padding: 4px 9px;
    border: 1px solid var(--hairline);
    font-size: 13px;
    color: var(--ink-soft);
    background: var(--surface);
  }

  .tag:hover {
    border-color: var(--link);
    color: var(--link);
  }

  .tag.here {
    border-color: var(--ink);
    color: var(--surface);
    background: var(--ink);
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

    .tools {
      width: 100%;
    }

    .search {
      flex: 1 1 100%;
      width: auto;
      min-width: 0;
      height: 44px;
    }

    .filter {
      height: 44px;
    }
  }
</style>
