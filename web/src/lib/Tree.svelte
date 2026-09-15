<script lang="ts">
  import { getFolders, getProblems, type FolderRow, type Issue, type Missing, type ProblemFiles } from './api';
  import { count, tagName } from './say';

  let {
    search,
    changed,
    missing,
    busy,
    onrescan,
  }: {
    search: string;
    changed: boolean;
    missing: Missing | null;
    busy: boolean;
    onrescan: (path: string) => void;
  } = $props();

  let listings = $state<Record<string, FolderRow[]>>({});
  let more = $state<Record<string, boolean>>({});
  let open = $state<string[]>([]);
  let failed = $state('');
  let waiting = $state<string[]>([]);
  /// The files behind one issue, keyed by folder and cause, fetched when it is opened.
  let behind = $state<Record<string, ProblemFiles | 'asking'>>({});

  // Only faults here. What the tags left unset is a diagnosis of its own and gets its own tab.
  const keyOf = (path: string, issue: Issue) => `${path}\u001f${issue.cause}`;

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

  const searching = $derived(search.trim().length > 0);

  async function load(path: string, wanted: string, only: boolean, lacking: Missing | null) {
    waiting = [...waiting, path];
    try {
      const listing = await getFolders(path, path === '' ? wanted : '', only, lacking);
      listings = { ...listings, [path]: listing.folders };
      more = { ...more, [path]: listing.more };
      failed = '';
    } catch (error) {
      failed = error instanceof Error ? error.message : String(error);
    } finally {
      waiting = waiting.filter((one) => one !== path);
    }
  }

  function toggle(row: FolderRow) {
    if (open.includes(row.path)) {
      open = open.filter((one) => one !== row.path);
      return;
    }
    open = [...open, row.path];
    if (!listings[row.path]) void load(row.path, '', changed, missing);
  }

  /// The rows as the page shows them: each open folder followed by what is under it.
  const rows = $derived.by(() => {
    const shown: { row: FolderRow; depth: number; last: boolean }[] = [];
    const walk = (under: string, depth: number) => {
      for (const row of listings[under] ?? []) {
        const unfolded = open.includes(row.path);
        shown.push({ row, depth, last: !unfolded });
        if (unfolded) walk(row.path, depth + 1);
      }
    };
    walk('', 0);
    return shown;
  });

  $effect(() => {
    const wanted = search.trim();
    const only = changed;
    const lacking = missing;
    const settle = setTimeout(
      () => {
        listings = {};
        more = {};
        open = [];
        void load('', wanted, only, lacking);
      },
      wanted ? 200 : 0,
    );
    return () => clearTimeout(settle);
  });
</script>

<div class="table">
  <div class="row heading dim">
    <span>Folder</span>
    <span class="right">Tracks</span>
    <span class="right">Albums</span>
    <span class="right">{missing ? `No ${tagName[missing]}` : 'Problems'}</span>
    <span></span>
  </div>

  {#if failed}
    <div class="note problem">{failed}</div>
  {:else if rows.length === 0}
    <div class="note dim">
      {#if waiting.includes('')}
        Reading the folders.
      {:else if searching}
        No folder of this library is named that.
      {:else if missing}
        Every track has {tagName[missing] === 'artwork' ? 'artwork' : `a ${tagName[missing]}`}.
      {:else if changed}
        Nothing was read again on the last check.
      {:else}
        This library holds no folders.
      {/if}
    </div>
  {/if}

  {#each rows as { row, depth } (row.path)}
    <div class="row" class:nested={depth > 0} style:--depth={depth}>
      <span class="folder">
        <button
          class="name mono"
          disabled={row.folders === 0}
          onclick={() => toggle(row)}
          title={row.path}
        >
          <span class="arrow">{row.folders === 0 ? '' : open.includes(row.path) ? '▾' : '▸'}</span>
          {searching ? row.path : row.name}
        </button>
        {#if row.changed}<span class="fresh">read again</span>{/if}
      </span>
      <span class="right mono">{count(row.tracks)}</span>
      <span class="right mono">{count(row.albums)}</span>
      {#if missing}
        <span class="right mono">{row.missing > 0 ? count(row.missing) : ''}</span>
      {:else}
        <span class="right mono" class:problem={row.problems > 0}>
          {row.problems > 0 ? count(row.problems) : ''}
        </span>
      {/if}
      <span class="says">
        <span class="holds">
          <span class="dim">{row.says}</span>
          {#each row.issues.filter((one) => one.problem) as issue (issue.cause)}
            {@const key = keyOf(row.path, issue)}
            <button
              class="issue"
              class:fault={issue.problem}
              class:shut={!issue.opens}
              disabled={!issue.opens}
              onclick={() => openIssue(row.path, issue)}
            >
              {issue.label}{issue.count > 0 ? `: ${count(issue.count)} ${issue.subject}` : ''}
            </button>
            {#if behind[key]}
              <span class="files mono dim">
                {#if behind[key] === 'asking'}
                  Reading.
                {:else if behind[key].shown.length === 0}
                  None of these were kept: only the first hundred of each are.
                {:else}
                  {#each behind[key].shown as one (one.subject)}
                    <span class="file">{one.subject}{one.detail ? ` — ${one.detail}` : ''}</span>
                  {/each}
                {/if}
              </span>
            {/if}
          {/each}
        </span>
        {#if row.tracks > 0 || row.problems > 0}
          <button class="quiet" disabled={busy} onclick={() => onrescan(row.path)}>
            Rescan folder
          </button>
        {/if}
      </span>
    </div>
  {/each}

  {#if more['']}
    <div class="note dim">
      More folders than one listing shows. Narrow it with the search box.
    </div>
  {/if}
</div>

<style>
  .table {
    display: flex;
    flex-direction: column;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 80px 80px 90px minmax(300px, 0.9fr);
    gap: 8px 16px;
    align-items: center;
    padding: 10px 0;
    border-bottom: 1px solid var(--hairline);
    font-size: 14px;
  }

  .row.nested {
    background: var(--row);
  }

  /* The name is indented, not the row: padding on the grid pushes the last column off the card. */
  .row.nested .folder {
    padding-left: calc(var(--depth) * 28px);
  }

  .heading {
    font-size: 12px;
    padding-bottom: 6px;
  }

  .right {
    text-align: right;
  }

  .folder {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .name {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    padding: 0;
    text-align: left;
    overflow-wrap: anywhere;
  }

  .name:disabled {
    cursor: default;
  }

  .name:not(:disabled):hover {
    color: var(--link);
  }

  .arrow {
    width: 10px;
    flex-shrink: 0;
    color: var(--muted);
  }

  .fresh {
    color: var(--link);
    font-size: 12px;
    white-space: nowrap;
  }

  .says {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    font-size: 13px;
    min-width: 0;
    line-height: 1.4;
  }

  .holds {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    min-width: 0;
  }

  .issue {
    padding: 1px 7px;
    border: 1px solid var(--hairline);
    font-size: 12px;
    color: var(--ink-soft);
    text-align: left;
  }

  .issue.fault {
    border-color: var(--warn-edge);
    color: var(--warn-ink);
  }

  .issue:not(:disabled):hover {
    border-color: var(--link);
    color: var(--link);
  }

  .issue.shut {
    border-style: dashed;
    cursor: default;
  }

  .files {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    padding: 2px 0 4px 8px;
    overflow-wrap: anywhere;
  }

  .note {
    padding: 14px 0;
    font-size: 13px;
  }

  @media (max-width: 720px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
      row-gap: 4px;
      padding: 14px 0;
    }

    .heading,
    .row .right:nth-child(3),
    .row .right:nth-child(4) {
      display: none;
    }

    .row .right:nth-child(2)::after {
      content: ' tracks';
      color: var(--muted);
    }

    .says {
      grid-column: 1 / -1;
      flex-direction: column;
      align-items: flex-start;
      gap: 6px;
    }

    .says .dim {
      white-space: normal;
    }

    .says .quiet {
      min-height: 44px;
    }
  }
</style>
