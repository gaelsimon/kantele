<script lang="ts">
  import { getProblems, type FolderAlbum, type FolderRow, type Issue, type ProblemFiles } from './api';
  import { count, many } from './say';

  let {
    row,
    albums,
    busy,
    onopen,
    onrescan,
  }: {
    row: FolderRow;
    /// What the files of this folder belong to, once its column has been read.
    albums: FolderAlbum[] | null;
    busy: boolean;
    onopen: (path: string, under: string) => void;
    onrescan: (path: string) => void;
  } = $props();

  /// The files behind one issue, keyed by folder and cause, fetched when it is opened.
  let behind = $state<Record<string, ProblemFiles | 'asking'>>({});

  const keyOf = (path: string, issue: Issue) => `${path}${issue.cause}`;

  async function openIssue(path: string, issue: Issue) {
    const key = keyOf(path, issue);
    if (key in behind) {
      const { [key]: _closed, ...rest } = behind;
      behind = rest;
      return;
    }
    behind = { ...behind, [key]: 'asking' };
    try {
      behind = { ...behind, [key]: await getProblems(path, issue.cause) };
    } catch {
      const { [key]: _failed, ...rest } = behind;
      behind = rest;
    }
  }
</script>

{#if !albums || albums.length === 0}
  <div class="cover" class:none={!row.artwork}>
    {#if row.artwork}
      <img src="/art/{row.artwork}" alt="" />
    {:else}
      <span class="dim">no cover</span>
    {/if}
  </div>
{/if}
<div class="sub">
  {row.says}{row.folders > 0 ? `, ${many(row.folders, 'folder')}` : ''}
</div>

{#if albums && albums.length > 0}
  <div class="sect">
    <div class="cap">{albums.length === 1 ? 'The album here' : 'The albums here'}</div>
    {#each albums as album (album.id)}
      <div class="album">
        <div class="cover small" class:none={!album.artwork}>
          {#if album.artwork}
            <img src="/art/{album.artwork}" alt="" />
          {:else}
            <span class="dim">no cover</span>
          {/if}
        </div>
        <div class="about">
          <div class="title">{album.title}</div>
          <div class="dim line">
            {[album.credit, album.date].filter(Boolean).join(' · ') || 'No album artist and no year'}
          </div>
          {#if album.says}<div class="line">{album.says}</div>{/if}
          {#if album.folders.length > 1}
            <div class="dim line mono">{album.folders.join(' · ')}</div>
          {/if}
          {#if album.cover_in}
            <div class="dim line">Its cover is in {album.cover_in}.</div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

{#if row.issues.length === 0}
  <div class="dim">This folder has no problem. The tags show nothing unusual.</div>
{:else}
  <div class="sect">
    <div class="cap">What the tags made of it</div>
    {#each row.issues as issue (issue.cause)}
      {@const key = keyOf(row.path, issue)}
      <button
        class="issue"
        class:fault={issue.problem}
        class:shut={!issue.opens}
        disabled={!issue.opens}
        onclick={() => openIssue(row.path, issue)}
      >
        <span class="tw">{issue.opens ? (key in behind ? '▾' : '▸') : ''}</span>
        {#if issue.problem}<span aria-hidden="true">▲</span>{/if}
        {issue.label}{issue.count > 0 ? `: ${count(issue.count)} ${issue.subject}` : ''}
      </button>
      {#if behind[key]}
        <div class="files mono">
          {#if behind[key] === 'asking'}
            <span class="dim">Reading.</span>
          {:else if behind[key].shown.length === 0}
            <span class="dim">The server keeps a maximum of one hundred examples of each problem. It did not keep these names.</span>
          {:else}
            {#each behind[key].shown as one (one.subject)}
              <button class="file" onclick={() => onopen(one.subject, row.path)}>
                {one.subject}{one.detail ? ` — ${one.detail}` : ''}
              </button>
            {/each}
          {/if}
        </div>
      {/if}
    {/each}
  </div>
{/if}

<!-- The library itself is read again from the card above, which says so in one place. -->
{#if row.path}
  <div class="acts">
    <button class="button" disabled={busy} onclick={() => onrescan(row.path)}>Rescan folder</button>
    <span class="dim aside">The server reads only this folder. It continues to serve the library.</span>
  </div>
{/if}

<style>
  .sub {
    font-size: 14px;
    color: var(--ink-soft);
    overflow-wrap: anywhere;
  }

  .issue {
    display: flex;
    align-items: baseline;
    gap: 6px;
    text-align: left;
    background: none;
    border: 0;
    padding: 2px 0;
    font: inherit;
    font-size: 13px;
    color: var(--ink-soft);
    cursor: pointer;
  }

  .issue:hover:not(:disabled) {
    color: var(--link);
  }

  .issue.fault {
    color: var(--warn-ink);
  }

  .issue.shut {
    cursor: default;
  }

  .issue .tw {
    width: 12px;
    flex-shrink: 0;
    color: var(--muted);
    font-size: 13px;
  }

  .files {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 2px 0 6px 16px;
    font-size: 12px;
  }

  .file {
    text-align: left;
    background: none;
    border: 0;
    padding: 0;
    font: inherit;
    color: var(--ink-soft);
    cursor: pointer;
    overflow-wrap: anywhere;
  }

  .file:hover {
    color: var(--link);
  }

  .album {
    display: flex;
    gap: 10px;
    padding: 6px 0;
    align-items: flex-start;
  }

  .about {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-size: 14px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .line {
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .acts {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-top: auto;
    padding-top: 10px;
    border-top: 1px solid var(--hairline);
  }

  .aside {
    font-size: 12px;
  }
</style>
