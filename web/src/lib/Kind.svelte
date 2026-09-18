<script lang="ts">
  /// What a row of a column is. Drawn rather than lettered: no glyph of a folder is in a system
  /// font, and the emoji pair is coloured and differently shaped on every platform.
  let { kind }: { kind: 'folder' | 'open' | 'note' | 'file' } = $props();
</script>

<svg
  class="kind"
  class:paper={kind === 'note' || kind === 'file'}
  viewBox="0 0 14 14"
  aria-hidden="true"
  focusable="false"
>
  {#if kind === 'folder'}
    <path
      d="M1.2 4.1c0-.6.5-1.1 1.1-1.1h3.1c.3 0 .5.1.7.3l.9 1h5c.6 0 1.1.5 1.1 1.1v5.2c0 .6-.5 1.1-1.1 1.1H2.3c-.6 0-1.1-.5-1.1-1.1z"
    />
  {:else if kind === 'open'}
    <path d="M1.2 4.1c0-.6.5-1.1 1.1-1.1h3.1c.3 0 .5.1.7.3l.9 1h5c.6 0 1.1.5 1.1 1.1v1.1H1.2z" />
    <path d="M1.2 7.1h11.6c.7 0 1.2.7 1 1.4l-1 3.1c-.1.5-.6.8-1.1.8H2.3c-.6 0-1.1-.5-1.1-1.1z" />
  {:else if kind === 'note'}
    <path
      d="M3.4 1.4h4.3l3.4 3.4v7.1c0 .6-.5 1.1-1.1 1.1H3.4c-.6 0-1.1-.5-1.1-1.1V2.5c0-.6.5-1.1 1.1-1.1z"
    />
    <path d="M8 1.6v2.6c0 .4.3.7.7.7h2.6z" fill="var(--surface)" />
    <path
      d="M8.5 7.4v3.3a1.05 1.05 0 1 1-.7-.99V8.6l-2 .42v2.24a1.05 1.05 0 1 1-.7-.99V7.82z"
      fill="var(--surface)"
    />
  {:else}
    <path d="M3.4 1.4h4.3l3.4 3.4v7.1c0 .6-.5 1.1-1.1 1.1H3.4c-.6 0-1.1-.5-1.1-1.1V2.5c0-.6.5-1.1 1.1-1.1z" />
    <path d="M8 1.6v2.6c0 .4.3.7.7.7h2.6z" fill="var(--surface)" />
  {/if}
</svg>

<style>
  .kind {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    align-self: center;
    fill: currentColor;
    color: var(--link);
    opacity: 0.7;
  }

  .paper {
    color: var(--faint);
    opacity: 1;
  }
</style>
