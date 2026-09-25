<script lang="ts">
  import type { FolderAlbum, FolderRow } from './api';
  import FilePane from './FilePane.svelte';
  import FolderPane from './FolderPane.svelte';
  import { leaf } from './selection';

  let {
    row,
    file,
    albums,
    busy,
    onopen,
    onback,
    onrescan,
    onfind,
  }: {
    /// The folder the pane is showing, which is the library itself when nothing is picked.
    row: FolderRow | null;
    /// The file the pane is showing, and the folder it was reached under.
    file: { path: string; under: string } | null;
    albums: FolderAlbum[] | null;
    busy: boolean;
    onopen: (path: string, under: string) => void;
    onback: () => void;
    onrescan: (path: string) => void;
    onfind: (name: string) => void;
  } = $props();
</script>

<aside class="pane">
  {#if file}
    <button class="crumb back mono" onclick={onback}>
      ‹ {file.under || 'the whole library'}
    </button>
    <h2 class="heading mono">{leaf(file.path)}</h2>
    <FilePane path={file.path} />
  {:else if row}
    {#if row.path}<div class="crumb mono">{row.path}</div>{/if}
    <h2 class="heading mono">{leaf(row.path) || 'The whole library'}</h2>
    <FolderPane {row} {albums} {busy} {onopen} {onrescan} {onfind} />
  {:else}
    <div class="empty dim">Reading the library.</div>
  {/if}
</aside>

<style>
  /* No frame of its own: the page draws one box around the columns and this. */
  .pane {
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: var(--surface);
    border-left: 1px solid var(--hairline);
    padding: 20px 22px;
    overflow-y: auto;
    min-height: 360px;
    max-height: min(60vh, 620px);
  }

  .empty {
    font-size: 13px;
    margin: auto 0;
  }

  .back {
    text-align: left;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
    color: var(--link);
  }

  .back:hover {
    text-decoration: underline;
  }

  @media (max-width: 720px) {
    .pane {
      max-height: none;
    }
  }
</style>
