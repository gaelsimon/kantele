<script lang="ts">
  import { getShares, type ShareEntry } from './api';

  let {
    images = false,
    start = '',
    onchoose,
    oncancel,
  }: {
    images?: boolean;
    start?: string;
    onchoose: (path: string) => void;
    oncancel: () => void;
  } = $props();

  // Settled by the first read, which starts wherever the field points.
  let under = $state('');
  let above = $state<string | null>(null);
  let entries = $state<ShareEntry[]>([]);
  let failed = $state('');
  let waiting = $state(true);

  async function open(where: string) {
    waiting = true;
    try {
      const listing = await getShares(where, images);
      under = listing.under;
      above = listing.above;
      entries = listing.entries;
      failed = '';
    } catch (error) {
      failed = error instanceof Error ? error.message : String(error);
      // A folder that is gone leaves the chooser at the shares rather than at nothing.
      if (where) void open('');
    } finally {
      waiting = false;
    }
  }

  $effect(() => {
    void open(start);
  });
</script>

<div class="picker">
  <div class="head">
    <span class="mono where">{under || 'Shared folders'}</span>
    <div class="acts">
      <button class="quiet" disabled={!above} onclick={() => open(above ?? '')}>Up</button>
      <button class="quiet" onclick={oncancel}>Cancel</button>
      {#if under && !images}
        <button class="button small" onclick={() => onchoose(under)}>Choose this folder</button>
      {/if}
    </div>
  </div>

  {#if failed}
    <div class="note problem">{failed}</div>
  {:else if waiting}
    <div class="note dim">Reading.</div>
  {:else if entries.length === 0}
    <div class="note dim">Nothing here to choose.</div>
  {/if}

  <ul>
    {#each entries as entry (entry.path)}
      <li>
        <button
          class="entry"
          onclick={() => (entry.folder ? open(entry.path) : onchoose(entry.path))}
        >
          <span class="glyph dim">{entry.folder ? '▸' : '·'}</span>
          <span class="mono">{entry.name}</span>
        </button>
        {#if entry.folder && !images}
          <button class="quiet" onclick={() => onchoose(entry.path)}>Choose</button>
        {/if}
      </li>
    {/each}
  </ul>
</div>

<style>
  .picker {
    border: 1px solid var(--faint);
    background: var(--surface);
    margin-top: 8px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--hairline);
    flex-wrap: wrap;
  }

  .where {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .acts {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .button.small {
    height: 30px;
    padding: 0 12px;
    font-size: 13px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 260px;
    overflow-y: auto;
  }

  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 12px;
    border-bottom: 1px solid var(--hairline);
  }

  li:last-child {
    border-bottom: 0;
  }

  .entry {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 9px 0;
    min-width: 0;
    text-align: left;
    flex: 1;
  }

  .entry:hover {
    color: var(--link);
  }

  .glyph {
    width: 10px;
    flex-shrink: 0;
  }

  .note {
    padding: 12px;
    font-size: 13px;
  }
</style>
