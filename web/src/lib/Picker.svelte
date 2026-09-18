<script lang="ts">
  import { getShares } from './api';
  import Columns from './Columns.svelte';
  import type { Entry, Level } from './columns';

  let {
    title,
    images = false,
    start = '',
    onchoose,
    oncancel,
  }: {
    title: string;
    images?: boolean;
    start?: string;
    onchoose: (path: string) => void;
    oncancel: () => void;
  } = $props();

  /// Nothing until the owner picks or types, so the chooser opens on what the field already holds.
  let typed = $state<string | null>(null);

  const chosen = $derived(typed ?? start);
  const selectable = $derived(chosen.trim() !== '');

  /// The disk, one level per ask, bounded to what this server may read.
  async function fill(path: string): Promise<Level<null>> {
    const listing = await getShares(path, images);
    return {
      entries: listing.entries.map((entry) => ({
        path: entry.path,
        name: entry.name,
        folder: entry.folder,
        of: null,
      })),
      more: listing.more,
    };
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') oncancel();
  }
</script>

<svelte:window {onkeydown} />

<!-- A column of this chooser lists folders, so an empty one is not an empty folder. -->
{#snippet nothing(at: number)}
  {#if at === 0}
    This server has no folder to show.
  {:else if images}
    This folder has no subfolder and no image.
  {:else}
    This folder has no subfolder.
  {/if}
{/snippet}

<!-- A click on the veil itself, beside the dialog, is a cancel; Escape is the key for it. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="veil"
  role="presentation"
  onclick={(event) => event.target === event.currentTarget && oncancel()}
>
  <div class="picker" role="dialog" aria-modal="true" aria-label={title}>
    <div class="head">
      <span class="title">{title}</span>
      <span class="dim sub">
        {images ? 'Images in the folders that this server can read' : 'Folders that this server can read'}
      </span>
    </div>

    <div class="list">
      <Columns
        load={fill}
        {start}
        tall="min(48vh, 420px)"
        selected={chosen}
        onpick={(entry: Entry<null>) => (typed = entry.path)}
        {nothing}
      />
    </div>

    <div class="foot">
      <input
        class="box mono"
        type="text"
        value={chosen}
        placeholder={images ? 'Path of an image' : 'Path of a folder'}
        oninput={(event) => (typed = event.currentTarget.value)}
      />
      <button class="button" onclick={oncancel}>Cancel</button>
      <button class="button strong" disabled={!selectable} onclick={() => onchoose(chosen.trim())}>
        Select
      </button>
    </div>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    background: rgba(20, 22, 25, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    z-index: 10;
  }

  .picker {
    background: var(--surface);
    border: 1px solid var(--edge);
    width: 100%;
    /* Wide, because the columns scroll sideways as the disk is walked. */
    max-width: 880px;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
  }

  .head {
    padding: 16px 20px;
    border-bottom: 1px solid var(--hairline);
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    flex-wrap: wrap;
  }

  .title {
    font-size: 15px;
    font-weight: 600;
  }

  .sub {
    font-size: 13px;
  }

  .list {
    padding: 12px 16px;
    overflow-y: auto;
    flex: 1;
    min-height: 120px;
  }

  .foot {
    padding: 12px 20px;
    border-top: 1px solid var(--hairline);
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
  }

  .box {
    flex: 1;
    min-width: 160px;
    height: 34px;
    border: 1px solid var(--faint);
    background: var(--surface);
    padding: 0 10px;
    font-size: 13px;
  }

  .button.strong {
    background: var(--ink);
    color: var(--surface);
    border-color: var(--ink);
  }

  .button.strong:hover:not(:disabled) {
    background: var(--ink-soft);
  }

  .button.strong:disabled {
    background: var(--hairline);
    border-color: var(--hairline);
    color: var(--faint);
  }

  @media (max-width: 720px) {
    .veil {
      padding: 0;
      align-items: stretch;
    }

    .picker {
      max-width: none;
      max-height: none;
      border: 0;
    }

    .box,
    .button {
      height: 44px;
    }
  }
</style>
