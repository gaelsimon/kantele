<script lang="ts">
  import type { Setting } from './api';
  import Picker from './Picker.svelte';

  export type Kind =
    | 'text'
    | 'number'
    | 'path'
    | 'paths'
    | 'words'
    | 'chips'
    | 'choice'
    | 'read';
  export type Shape = { kind: Kind; unit?: string; note?: string; images?: boolean };

  let {
    setting,
    shape,
    value,
    changed,
    onchange,
  }: {
    setting: Setting;
    shape: Shape;
    value: unknown;
    changed: boolean;
    onchange: (value: unknown) => void;
  } = $props();

  let picking = $state<number | null>(null);

  const held = $derived(!setting.writable);
  const paths = $derived(Array.isArray(value) ? (value as string[]) : value ? [String(value)] : []);
  const words = $derived(Array.isArray(value) ? (value as string[]).join(', ') : '');
  const chosen = $derived(Array.isArray(value) ? (value as string[]) : []);

  /// Where a value nobody can change from here comes from.
  const from = $derived.by(() => {
    switch (setting.source.layer) {
      case 'environment':
        return `set by ${setting.source.variable}`;
      case 'command-line':
        return 'named on the command line';
      default:
        return setting.says;
    }
  });

  function asWords(text: string) {
    return text
      .split(',')
      .map((word) => word.trim())
      .filter(Boolean);
  }

  function replace(at: number, path: string) {
    const next = [...paths];
    next[at] = path;
    onchange(next);
  }
</script>

<div class="field" class:changed>
  <span class="label" class:mark={changed}>{setting.label}</span>

  <div class="control">
    {#if held || shape.kind === 'read'}
      <div class="held mono">{setting.value || '—'}</div>
      <div class="dim note">{from}</div>
    {:else if shape.kind === 'number'}
      <div class="row">
        <input
          class="box num"
          type="number"
          min="0"
          value={typeof value === 'number' ? value : 0}
          oninput={(event) => onchange(Number(event.currentTarget.value))}
        />
        {#if shape.unit}<span class="dim unit">{shape.unit}</span>{/if}
      </div>
    {:else if shape.kind === 'text'}
      <input
        class="box"
        type="text"
        value={typeof value === 'string' ? value : ''}
        oninput={(event) => onchange(event.currentTarget.value)}
      />
    {:else if shape.kind === 'choice'}
      <select
        class="box pick"
        value={typeof value === 'string' ? value : ''}
        onchange={(event) => onchange(event.currentTarget.value)}
      >
        {#each setting.choices ?? [] as choice (choice.value)}
          <option value={choice.value}>{choice.label}</option>
        {/each}
      </select>
    {:else if shape.kind === 'words'}
      <input
        class="box mono"
        type="text"
        value={words}
        placeholder="none"
        oninput={(event) => onchange(asWords(event.currentTarget.value))}
      />
    {:else if shape.kind === 'path'}
      <div class="row">
        <input
          class="box mono"
          type="text"
          value={typeof value === 'string' ? value : ''}
          placeholder="none"
          oninput={(event) => onchange(event.currentTarget.value || null)}
        />
        <button class="quiet" onclick={() => (picking = picking === 0 ? null : 0)}>
          {picking === 0 ? 'Close' : 'Choose'}
        </button>
      </div>
      {#if picking === 0}
        <Picker
          images={shape.images ?? false}
          start={typeof value === 'string' ? value : ''}
          onchoose={(path) => {
            onchange(path);
            picking = null;
          }}
          oncancel={() => (picking = null)}
        />
      {/if}
    {:else if shape.kind === 'paths'}
      {#each paths as path, at (at)}
        <div class="row">
          <input
            class="box mono"
            type="text"
            value={path}
            oninput={(event) => replace(at, event.currentTarget.value)}
          />
          <button class="quiet" onclick={() => (picking = picking === at ? null : at)}>
            {picking === at ? 'Close' : 'Choose'}
          </button>
          <button
            class="quiet"
            disabled={paths.length < 2}
            onclick={() => onchange(paths.filter((_, one) => one !== at))}
          >
            Remove
          </button>
        </div>
        {#if picking === at}
          <Picker
            start={path}
            onchoose={(chosenPath) => {
              replace(at, chosenPath);
              picking = null;
            }}
            oncancel={() => (picking = null)}
          />
        {/if}
      {/each}
      <button class="button small" onclick={() => onchange([...paths, ''])}>Add folder</button>
    {:else if shape.kind === 'chips'}
      <div class="chips">
        {#each chosen as code, at (code)}
          {@const choice = setting.choices?.find((one) => one.value === code)}
          <span class="chip">
            <button
              class="move"
              disabled={at === 0}
              title="Earlier"
              onclick={() => {
                const next = [...chosen];
                const moved = next.splice(at, 1);
                onchange([...next.slice(0, at - 1), ...moved, ...next.slice(at - 1)]);
              }}>←</button
            >
            {choice?.label ?? code}
            <button class="move" title="Remove" onclick={() => onchange(chosen.filter((one) => one !== code))}>×</button>
          </span>
        {/each}
        {#each setting.choices ?? [] as choice (choice.value)}
          {#if !chosen.includes(choice.value)}
            <button class="chip add" onclick={() => onchange([...chosen, choice.value])}>
              + {choice.label}
            </button>
          {/if}
        {/each}
      </div>
    {/if}

    {#if shape.note && !held && shape.kind !== 'read'}
      <div class="dim note">{shape.note}</div>
    {/if}
  </div>
</div>

<style>
  .field {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr);
    gap: 6px 14px;
    align-items: baseline;
    padding: 7px 0;
    font-size: 14px;
  }

  .label {
    color: var(--ink-soft);
    overflow-wrap: anywhere;
  }

  .label.mark {
    color: var(--link);
  }

  .control {
    min-width: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .row + .row {
    margin-top: 6px;
  }

  .box {
    height: 34px;
    border: 1px solid var(--faint);
    background: var(--surface);
    padding: 0 10px;
    font-size: 13px;
    width: 100%;
    min-width: 0;
  }

  .box.num {
    width: 100px;
    text-align: right;
  }

  .box.pick {
    width: auto;
    max-width: 100%;
    height: 34px;
    cursor: pointer;
  }

  .changed .box {
    border-color: var(--link);
    border-left-width: 3px;
  }

  /* It grows: a path is as long as it is, and a fixed height would run it over the line below. */
  .held {
    min-height: 34px;
    border: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    padding: 6px 10px;
    font-size: 13px;
    line-height: 1.4;
    color: var(--faint);
    overflow-wrap: anywhere;
  }

  .unit {
    font-size: 13px;
  }

  .note {
    font-size: 12px;
    margin-top: 4px;
  }

  .button.small {
    height: 32px;
    padding: 0 14px;
    font-size: 13px;
    margin-top: 8px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 8px;
    border: 1px solid var(--faint);
    font-size: 13px;
    background: var(--surface);
  }

  .chip.add {
    border-style: dashed;
    color: var(--muted);
  }

  .chip.add:hover {
    color: var(--link);
    border-color: var(--link);
  }

  .move {
    color: var(--muted);
    font-size: 12px;
    line-height: 1;
    padding: 0 2px;
  }

  .move:hover:not(:disabled) {
    color: var(--link);
  }

  .move:disabled {
    color: var(--hairline);
    cursor: default;
  }

  @media (max-width: 720px) {
    .field {
      grid-template-columns: minmax(0, 1fr);
    }

    .box {
      height: 44px;
    }

    .box.num {
      width: 120px;
    }
  }
</style>
