<script lang="ts">
  import type { Declared, Flag } from './api';

  let {
    name,
    glyph,
    warn = false,
    boxes,
    ticked,
    ontick,
  }: {
    name: string;
    glyph: string;
    warn?: boolean;
    boxes: Declared[];
    ticked: Flag[];
    ontick: (flags: Flag[], on: boolean) => void;
  } = $props();

  let open = $state(false);
  let root: HTMLElement | undefined = $state();

  const mine = $derived(boxes.filter((box) => ticked.includes(box.flag)));
  const every = $derived(boxes.map((box) => box.flag));
  const says = $derived(
    mine.length === 1 ? `${name}: ${mine[0]?.label}` : mine.length > 1 ? `${name} · ${mine.length}` : name,
  );

  $effect(() => {
    if (!open) return;
    const away = (event: MouseEvent) => {
      if (root && !root.contains(event.target as Node)) open = false;
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') open = false;
    };
    document.addEventListener('click', away);
    document.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('click', away);
      document.removeEventListener('keydown', key);
    };
  });
</script>

<span class="filter" bind:this={root}>
  <button
    class="filter-chip"
    class:on={mine.length > 0}
    class:joined={mine.length > 0}
    aria-expanded={open}
    aria-haspopup="true"
    onclick={() => (open = !open)}
  >
    <span class="glyph" class:warn aria-hidden="true">{glyph}</span>
    {says}
    {#if mine.length === 0}<span class="caret" aria-hidden="true">▾</span>{/if}
  </button>
  {#if mine.length > 0}
    <button class="filter-chip on clear" aria-label="Clear {name}" onclick={() => ontick(every, false)}>×</button>
  {/if}
  {#if open}
    <div class="menu">
      <div class="top">
        <span class="cap">{name}</span>
        <button class="quiet" onclick={() => ontick(every, mine.length < boxes.length)}>
          {mine.length < boxes.length ? 'All' : 'None'}
        </button>
      </div>
      {#each boxes as box (box.flag)}
        <label class="opt" class:apart={box.apart}>
          <input
            type="checkbox"
            aria-label={box.label}
            aria-describedby="{name}-{box.flag}"
            checked={ticked.includes(box.flag)}
            onchange={(event) => ontick([box.flag], event.currentTarget.checked)}
          />
          <span>{box.label}</span>
          <span class="say" id="{name}-{box.flag}">{box.says}</span>
        </label>
      {/each}
    </div>
  {/if}
</span>

<style>
  .filter {
    position: relative;
    display: inline-flex;
  }

  .glyph {
    font-size: 11px;
    color: var(--muted);
  }

  .glyph.warn {
    color: var(--warn);
  }

  .caret {
    font-size: 11px;
    color: var(--muted);
  }

  .joined {
    border-right: 0;
  }

  .clear {
    padding: 0 10px;
    font-size: 16px;
    color: var(--muted);
  }

  .clear:hover {
    color: var(--ink);
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 5;
    width: 340px;
    max-width: calc(100vw - 32px);
    background: var(--surface);
    border: 1px solid var(--edge);
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.18);
    padding: 6px 0;
  }

  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 14px 8px;
    border-bottom: 1px solid var(--hairline);
  }

  .opt {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    column-gap: 8px;
    align-items: baseline;
    padding: 7px 14px;
    font-size: 14px;
    cursor: pointer;
  }

  .opt:hover {
    background: var(--row);
  }

  .opt input {
    margin: 0;
    transform: translateY(2px);
  }

  .opt.apart {
    border-top: 1px solid var(--hairline);
    margin-top: 4px;
    padding-top: 11px;
  }

  .say {
    grid-column: 2;
    font-size: 12px;
    line-height: 1.35;
    color: var(--muted);
  }

  @media (prefers-reduced-motion: no-preference) {
    .menu {
      animation: drop 120ms ease-out;
    }

    @keyframes drop {
      from {
        opacity: 0;
        transform: translateY(-4px);
      }
    }
  }
</style>
