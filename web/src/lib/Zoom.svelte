<script lang="ts">
  let { src }: { src: string } = $props();

  let open = $state(false);
  let size = $state<{ width: number; height: number } | null>(null);
</script>

<svelte:window
  onkeydown={(event) => {
    if (open && event.key === 'Escape') open = false;
  }}
/>

<button class="zoom" title="Show at its own size" onclick={() => (open = true)}>
  <img {src} alt="" />
</button>

{#if open}
  <!-- Its own size, as far as the window allows, which is what a player gets to show. -->
  <button class="shown" aria-label="Close the cover" onclick={() => (open = false)}>
    <img
      {src}
      alt=""
      onload={(event) => {
        const { naturalWidth: width, naturalHeight: height } = event.currentTarget as HTMLImageElement;
        size = { width, height };
      }}
    />
    {#if size}<span class="size num">{size.width} × {size.height}</span>{/if}
  </button>
{/if}

<style>
  .zoom {
    display: block;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    cursor: zoom-in;
  }

  .shown {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 24px;
    border: 0;
    background: rgb(0 0 0 / 0.75);
    cursor: zoom-out;
  }

  .shown img {
    max-width: 100%;
    max-height: calc(100vh - 80px);
    width: auto;
    height: auto;
  }

  .size {
    color: #fff;
    font-size: 13px;
  }
</style>
