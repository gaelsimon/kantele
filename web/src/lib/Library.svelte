<script lang="ts">
  import {
    getFolders,
    rescan,
    type Configuration,
    type FolderRow,
    type Missing,
    type Status,
  } from './api';
  import { ago, count, many, noun, tagName, took } from './say';
  import Tree from './Tree.svelte';

  let {
    status,
    configuration,
    onrescanned,
  }: {
    status: Status | null;
    configuration: Configuration | null;
    onrescanned: () => void;
  } = $props();

  let search = $state('');
  let changedFirst = $state(false);
  let lacking = $state<Missing | null>(null);
  let here = $state<FolderRow | null>(null);
  let asking = $state(false);
  let said = $state('');

  const folder = $derived(
    configuration?.settings.find((setting) => setting.key === 'content_dir')?.value ?? '',
  );

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
          ? 'The index was replaced.'
          : (pass.why ?? '');
    return `${ran} ${became}`.trim();
  });

  /// When the server next looks of its own accord, where it does.
  const nextCheck = $derived.by(() => {
    const minutes = configuration?.settings.find(
      (setting) => setting.key === 'scan.sweep_minutes',
    )?.as_written;
    if (typeof minutes !== 'number' || minutes === 0) return 'It only looks when something changes.';
    const last = status?.last_pass?.ended;
    if (!last) return `It looks every ${minutes} min.`;
    const due = Math.round((last + minutes * 60 - Date.now() / 1000) / 60);
    return due > 0 ? `Next in ${due} min.` : 'Next check is due.';
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

  async function readHere() {
    try {
      here = (await getFolders('', '', false)).here;
    } catch {
      here = null;
    }
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

  $effect(() => {
    void readHere();
  });
</script>

<div class="boards">
  <section class="card serving">
    <div class="cap">Serving</div>
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
      <span class="dim aside">{said || 'serving meanwhile'}</span>
    </div>
  </section>
</div>

<section class="card tree">
  <div class="head">
    <div class="mono where">{folder}</div>
    <div class="tools">
      <input class="search" type="search" placeholder="Search folders" bind:value={search} />
      <label class="filter">
        <input type="checkbox" bind:checked={changedFirst} />
        Changed only
      </label>
    </div>
  </div>
  {#if here}
    <div class="dim totals">
      {many(here.tracks, 'track')} · {many(here.albums, 'album')} · {many(here.problems, 'problem')}
    </div>
  {/if}

  <Tree {search} changed={changedFirst} missing={lacking} busy={asking} onrescan={askForAPass} />

  {#if missing.length > 0}
    <div class="missing">
      <span class="dim">Missing tags</span>
      {#each missing as one (one.of)}
        <button
          class="tag"
          class:here={lacking === one.of}
          onclick={() => (lacking = lacking === one.of ? null : one.of)}
        >
          {count(one.n)} no {tagName[one.of]}
        </button>
      {/each}
      {#if lacking}
        <button class="quiet" onclick={() => (lacking = null)}>All folders</button>
      {/if}
    </div>
  {:else if status && status.library.tracks > 0}
    <div class="dim missing">Every track has a date, a genre, an artist and artwork.</div>
  {/if}
</section>

<style>
  .boards {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr);
    gap: 24px;
    margin-bottom: 24px;
  }

  .serving,
  .check {
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

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    flex-wrap: wrap;
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

  .totals,
  .missing {
    font-size: 13px;
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
