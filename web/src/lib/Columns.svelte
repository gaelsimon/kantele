<script lang="ts" generics="T">
  import { untrack, type Snippet } from 'svelte';
  import Kind from './Kind.svelte';
  import { halves, stepped, type Entry, type Level } from './columns';
  import { Walk } from './walk.svelte';

  let {
    load,
    reload = 0,
    start = '',
    tall = 'min(60vh, 620px)',
    framed = true,
    selected = null,
    onpick,
    before,
    beside,
    mark,
    nothing,
    bounded,
  }: {
    /// What fills one column. `first` marks the column the listing opens on, which is where a
    /// search or a filter applies.
    load: (path: string, first: boolean) => Promise<Level<T>>;
    /// Anything that makes every column stale. A new value empties them and fills them again.
    reload?: number;
    /// A path to open down to when the columns first appear.
    start?: string;
    /// How tall the columns stand. One height for as long as they are open, whatever they hold.
    tall?: string;
    /// Whether the columns draw their own frame, or sit inside one the page draws.
    framed?: boolean;
    /// The path the page is showing, which the column draws as picked.
    selected?: string | null;
    onpick: (entry: Entry<T>) => void;
    /// What is drawn before the name of a row, beside it, and at the right of the column.
    before?: Snippet<[Entry<T>]>;
    beside?: Snippet<[Entry<T>]>;
    mark?: Snippet<[Entry<T>]>;
    /// What a column with nothing in it says, by its place in the trail.
    nothing?: Snippet<[number]>;
    /// What a column holding more than one answer carries says, where the page can say better.
    bounded?: Snippet<[number]>;
  } = $props();

  /// Another source, or another library, is another walk. The columns empty and fill again.
  const walk = $derived.by(() => {
    reload;
    return new Walk(load);
  });

  let active = $state<{ column: number; index: number } | null>(null);
  let buttons: Record<string, HTMLButtonElement | undefined> = {};
  let strip = $state<HTMLElement | null>(null);

  const columns = $derived(walk.columns);
  const lists = $derived(columns.map((under) => walk.entries(under)));

  function pick(column: number, index: number) {
    const entry = lists[column]?.[index];
    if (!entry) return;
    active = { column, index };
    if (entry.folder) walk.open(column, entry.path);
    else walk.close(column);
    onpick(entry);
  }

  /// Left and right change column, up and down move inside one.
  function key(event: KeyboardEvent) {
    if (active === null) return;
    const { column, index } = active;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      pick(column, stepped(index, event.key === 'ArrowDown' ? 1 : -1, lists[column]?.length ?? 0));
      return;
    }
    if (event.key === 'ArrowRight' && lists[column + 1]?.length) {
      event.preventDefault();
      pick(column + 1, 0);
      return;
    }
    if (event.key === 'ArrowLeft' && column > 0) {
      event.preventDefault();
      const at = (lists[column - 1] ?? []).findIndex((one) => one.path === columns[column]);
      pick(column - 1, Math.max(0, at));
    }
  }

  function back() {
    if (walk.trail.length === 0) return;
    const column = walk.trail.length - 1;
    const row = lists[column]?.find((one) => one.path === columns[column + 1]);
    walk.close(column);
    active = null;
    if (row) onpick(row);
  }

  $effect(() => {
    const walking = walk;
    untrack(() => {
      active = null;
      void walking.restart().then(() => {
        if (start) return walking.revealTo(start);
      });
    });
  });

  // A column opened to the right is a column to scroll to.
  $effect(() => {
    walk.trail.length;
    if (strip) strip.scrollLeft = strip.scrollWidth;
  });

  $effect(() => {
    if (active) buttons[`${active.column}:${active.index}`]?.focus();
  });
</script>

<div class="frame">
  <button class="quiet back" disabled={walk.trail.length === 0} onclick={back}>‹ Back</button>

  <div
    class="strip"
    class:bare={!framed}
    bind:this={strip}
    style:height={tall}
    role="group"
    aria-label="One column per folder"
  >
    {#each columns as under, at (under)}
      {@const list = lists[at] ?? []}
      <div class="col" class:last={at === columns.length - 1}>
        {#each list as entry, index (entry.path)}
          {@const [head, tail] = halves(entry.name)}
          <button
            class="entry"
            class:open={columns[at + 1] === entry.path}
            class:here={selected === entry.path}
            bind:this={buttons[`${at}:${index}`]}
            title={entry.path}
            onclick={() => pick(at, index)}
            onfocus={() => (active = { column: at, index })}
            onkeydown={key}
          >
            <Kind
              kind={entry.folder
                ? columns[at + 1] === entry.path
                  ? 'open'
                  : 'folder'
                : (entry.icon ?? 'file')}
            />
            <span class="what">
              {@render before?.(entry)}
              <span class="name" class:file={!entry.folder}>
                <span class="head">{head}</span>{#if tail}<span class="tail">{tail}</span>{/if}
              </span>
              {@render beside?.(entry)}
            </span>
            {@render mark?.(entry)}
            {#if entry.folder}<span class="twist" aria-hidden="true">›</span>{/if}
          </button>
        {/each}

        {#if walk.broke[under]}
          <div class="note problem">{walk.broke[under]}</div>
          <button class="quiet again" onclick={() => void walk.read(under)}>Read it again</button>
        {:else if walk.waiting.includes(under) && list.length === 0}
          <div class="note dim">Reading.</div>
        {:else if walk.levels[under] && list.length === 0}
          <div class="note dim">{@render nothing?.(at)}</div>
        {/if}

        {#if walk.levels[under]?.more}
          <div class="note dim">
            {#if bounded}{@render bounded(at)}{:else}The list does not show all of them.{/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .frame {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  /* One height for as long as it is open. Following the content made it jump twice on every
     click, once while the column was reading and once when it had read. */
  .strip {
    display: flex;
    flex: 1 1 auto;
    overflow-x: auto;
    border: 1px solid var(--hairline);
    background: var(--surface);
    min-height: 240px;
  }

  .strip.bare {
    border: 0;
  }

  /* Every column shares the width there is, up to a point: past it a line of names is harder to
     read, not easier. What is left over goes to the last column, which is the one being read. */
  .col {
    flex: 1 1 248px;
    min-width: 210px;
    max-width: 420px;
    overflow-y: auto;
    border-right: 1px solid var(--hairline);
    padding: 4px 0;
  }

  .col.last {
    max-width: none;
    border-right: 0;
  }

  .entry {
    display: flex;
    align-items: baseline;
    gap: 6px;
    width: 100%;
    text-align: left;
    padding: 5px 8px 5px 12px;
    font-size: 13px;
    line-height: 1.3;
  }

  .entry:hover {
    background: var(--row);
  }

  .entry.open {
    background: var(--hairline);
  }

  .entry.here {
    background: var(--link-paper);
    color: var(--link);
  }

  .entry:focus-visible {
    outline: 2px solid var(--link);
    outline-offset: -2px;
  }

  .what {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    flex: 1 1 auto;
  }

  /* The end of a name is what tells two folders apart, so the middle is what gives way. */
  .name {
    display: flex;
    min-width: 0;
  }

  /* `pre`, not `nowrap`: the space at the seam is the one between two words of the name. */
  .head {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: pre;
  }

  .tail {
    flex-shrink: 0;
    white-space: pre;
  }

  .file {
    color: var(--ink-soft);
  }

  .entry.here .file {
    color: inherit;
  }

  .twist {
    color: var(--faint);
    font-size: 13px;
  }

  .note {
    padding: 8px 12px;
    font-size: 12px;
  }

  .again {
    padding: 0 12px;
  }

  .back {
    display: none;
    align-self: flex-start;
  }

  @media (max-width: 720px) {
    /* A column means nothing on a phone: one level at a time, and a way back up. */
    .back {
      display: inline;
    }

    .col {
      flex: 1 1 100%;
      border-right: 0;
    }

    .col:not(.last) {
      display: none;
    }

    .strip {
      height: 60vh;
      min-height: 0;
    }
  }
</style>
