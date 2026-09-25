<script lang="ts">
  import Zoom from './Zoom.svelte';
  import { getTrack, type TrackDetail } from './api';
  import { length, size } from './say';

  let { path }: { path: string } = $props();

  let found = $state<TrackDetail | 'asking' | 'gone' | null>(null);

  /// One request per file shown, and a late answer for a file nobody is looking at any more is
  /// dropped rather than drawn.
  let wanted = $state('');
  $effect(() => {
    const asked = path;
    wanted = asked;
    found = 'asking';
    getTrack(asked)
      .then((track) => {
        if (wanted === asked) found = track;
      })
      .catch(() => {
        // A file the library holds no track for is the whole answer: its tags would not read.
        if (wanted === asked) found = 'gone';
      });
  });
</script>

{#if found === 'asking'}
  <div class="dim">Reading.</div>
{:else if found === 'gone'}
  <div class="dim">The library has no entry for this file. The server could not read its tags.</div>
{:else if found}
  <div class="cover" class:none={!found.artwork} class:whole={found.artwork}>
    {#if found.artwork}
      <Zoom src="/art/{found.artwork}" />
    {:else}
      <span class="dim">no cover</span>
    {/if}
  </div>
  <div class="summary">{found.format} · {size(found.bytes)} · {length(found.seconds)}</div>
  <div class="sect">
    <div class="cap">Tags</div>
    <div class="kv">
      {#each found.tags as tag (tag.label)}
        <span class="k">{tag.label}</span>
        {#if tag.value}
          <span>{tag.value}</span>
        {:else}
          <span class="absent">not set</span>
        {/if}
      {/each}
      <span class="k">File</span>
      <span>{found.format}</span>
    </div>
  </div>
  <div class="sect">
    <div class="cap">On your players</div>
    {#if found.places.length === 0}
      <span class="absent">The menus that you chose do not show this file.</span>
    {:else}
      <div class="where">
        {#if found.album}
          <span class="path">
            Album › <b>{found.album.title}</b>{found.album.keyed_on_path ? ' (named by folder)' : ''}
          </span>
        {/if}
        {#each found.places as place (place.at)}
          <span class="path">{place.axis} › <b>{place.value}</b></span>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  /* The cover the file carries, as wide as the pane: the one picture anyone looks at here. */
  .cover.whole {
    width: 100%;
  }

  .summary {
    font-size: 13px;
    color: var(--muted);
  }

  /* One line per tag, parted by a hairline, the value to the right as a file browser sets it. */
  .kv {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    align-items: baseline;
    font-size: 13px;
  }

  .kv > span {
    padding: 5px 0;
    border-bottom: 1px solid var(--hairline);
  }

  .kv > span:nth-child(even) {
    padding-left: 16px;
    text-align: right;
  }

  .k {
    color: var(--muted);
    font-size: 12px;
  }

  .absent {
    color: var(--warn-ink);
    font-size: 13px;
  }

  .where {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .path {
    font-size: 13px;
    padding: 2px 8px;
    border: 1px solid var(--hairline);
    background: var(--row);
    color: var(--ink-soft);
  }

  .path b {
    font-weight: 500;
    color: var(--ink);
  }

  @media (max-width: 720px) {
    .kv {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
