<script lang="ts">
  import type { Setting } from './api';
  import { costClass } from './apply';
  import type { Shape } from './settings';
  import Picker from './Picker.svelte';

  let {
    setting,
    shape,
    value,
    changed,
    tagged,
    onchange,
  }: {
    setting: Setting;
    shape: Shape;
    value: unknown;
    changed: boolean;
    /// Whether the field carries its own cost tag, because the section's badge does not speak for it.
    tagged: boolean;
    onchange: (value: unknown) => void;
  } = $props();

  let picking = $state<number | null>(null);
  let dragging = $state<number | null>(null);

  const held = $derived(!setting.writable || shape.kind === 'read');
  const paths = $derived(Array.isArray(value) ? (value as string[]) : value ? [String(value)] : []);
  const words = $derived(Array.isArray(value) ? (value as string[]).join(', ') : '');
  const chosen = $derived(Array.isArray(value) ? (value as string[]) : []);
  const path = $derived(typeof value === 'string' ? value : '');

  /// Which layer holds a value nobody can change from here.
  const from = $derived.by(() => {
    switch (setting.source.layer) {
      case 'environment':
        return `set by ${setting.source.variable}`;
      case 'command-line':
        return 'named on the command line';
      case 'file':
        return 'from the settings file';
      default:
        return 'the default';
    }
  });

  function asWords(text: string) {
    return text
      .split(',')
      .map((word) => word.trim())
      .filter(Boolean);
  }

  function replace(at: number, chosenPath: string) {
    const next = [...paths];
    next[at] = chosenPath;
    onchange(next);
  }

  function labelOf(code: string) {
    return setting.choices?.find((one) => one.value === code)?.label ?? code;
  }

  function moved(fromAt: number, toAt: number) {
    if (fromAt === toAt) return;
    const next = [...chosen];
    next.splice(toAt, 0, ...next.splice(fromAt, 1));
    onchange(next);
  }
</script>

<div class="field" class:changed>
  <span class="label" class:mark={changed}>
    {setting.label}
    {#if tagged && setting.apply !== 'never'}
      <span class="cost {costClass(setting.apply)}" title={setting.says}>{setting.tag}</span>
    {/if}
  </span>

  <div class="control">
    {#if held}
      <div class="held">
        <span class="val" class:mono={shape.kind === 'read' && setting.key !== 'clients'}
          >{setting.value || '—'}</span
        >
        <span class="dim from">{from}</span>
      </div>
    {:else if shape.kind === 'number'}
      <div class="row">
        {#if shape.before}<span class="dim word">{shape.before}</span>{/if}
        <input
          class="box num"
          type="number"
          min="0"
          value={typeof value === 'number' ? value : 0}
          oninput={(event) => {
            // An empty box is a number being typed, and zero turns several of these off.
            const typed = event.currentTarget.valueAsNumber;
            if (Number.isInteger(typed) && typed >= 0) onchange(typed);
          }}
        />
        {#if shape.after}<span class="dim word">{shape.after}</span>{/if}
      </div>
    {:else if shape.kind === 'text'}
      <input
        class="box short"
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
    {:else if shape.kind === 'image' || shape.kind === 'folder'}
      <div class="held">
        <span class="val" class:mono={path !== ''}>
          {path || (shape.kind === 'image' ? 'Built in' : 'Off')}
        </span>
        <button class="quiet" onclick={() => (picking = picking === 0 ? null : 0)}>
          {#if picking === 0}Close{:else if shape.kind === 'image'}Select an image{:else}Select a folder{/if}
        </button>
        {#if path}
          <button class="quiet" onclick={() => onchange(null)}>
            {shape.kind === 'image' ? 'Use the built-in icon' : 'Turn off'}
          </button>
        {/if}
      </div>
      {#if picking === 0}
        <Picker
          title={setting.label}
          images={shape.kind === 'image'}
          start={path}
          onchoose={(chosenPath) => {
            onchange(chosenPath);
            picking = null;
          }}
          oncancel={() => (picking = null)}
        />
      {/if}
    {:else if shape.kind === 'paths'}
      {#each paths as one, at (at)}
        <div class="row">
          <input
            class="box mono"
            type="text"
            value={one}
            oninput={(event) => replace(at, event.currentTarget.value)}
          />
          <button class="quiet" onclick={() => (picking = picking === at ? null : at)}>
            {picking === at ? 'Close' : 'Change'}
          </button>
          {#if paths.length > 1}
            <button class="quiet" onclick={() => onchange(paths.filter((_, other) => other !== at))}>
              Remove
            </button>
          {/if}
        </div>
        {#if picking === at}
          <Picker
            title={setting.label}
            start={one}
            onchoose={(chosenPath) => {
              replace(at, chosenPath);
              picking = null;
            }}
            oncancel={() => (picking = null)}
          />
        {/if}
      {/each}
      <button
        class="quiet add"
        onclick={() => {
          onchange([...paths, '']);
          picking = paths.length;
        }}
      >
        Add a folder
      </button>
    {:else if shape.kind === 'chips'}
      <div class="chips" role="list">
        {#each chosen as code, at (code)}
          <span
            class="chip"
            class:lifted={dragging === at}
            role="listitem"
            draggable="true"
            ondragstart={(event) => {
              dragging = at;
              event.dataTransfer?.setData('text/plain', code);
            }}
            ondragover={(event) => event.preventDefault()}
            ondrop={(event) => {
              event.preventDefault();
              if (dragging !== null) moved(dragging, at);
              dragging = null;
            }}
            ondragend={() => (dragging = null)}
          >
            <span class="grip" aria-hidden="true">⋮⋮</span>
            <button
              class="move"
              disabled={at === 0}
              title="Earlier"
              onclick={() => moved(at, at - 1)}>←</button
            >
            {labelOf(code)}
            <button
              class="move"
              title="Remove"
              onclick={() => onchange(chosen.filter((one) => one !== code))}>×</button
            >
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

    {#if setting.note || (shape.note && !held)}
      <div class="dim note">{[setting.note, held ? '' : shape.note].filter(Boolean).join(' ')}</div>
    {/if}
  </div>
</div>

<style>
  .field {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr);
    gap: 6px 14px;
    align-items: baseline;
    padding: 8px 0;
    font-size: 14px;
  }

  .label {
    color: var(--ink-soft);
    overflow-wrap: anywhere;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }

  .label.mark {
    color: var(--link);
  }

  .label .cost {
    font-size: 11px;
    padding: 1px 6px;
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

  .word {
    font-size: 13px;
    white-space: nowrap;
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

  .box.short {
    width: 240px;
    max-width: 100%;
  }

  .box.num {
    width: 88px;
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

  .held {
    display: flex;
    gap: 10px;
    align-items: baseline;
    flex-wrap: wrap;
    font-size: 13px;
    color: var(--muted);
    line-height: 1.6;
  }

  .held .val {
    color: var(--ink-soft);
    overflow-wrap: anywhere;
  }

  .from {
    font-size: 12px;
  }

  .note {
    font-size: 12px;
    margin-top: 4px;
  }

  .quiet.add {
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
    cursor: grab;
  }

  .chip.lifted {
    opacity: 0.5;
  }

  .chip .grip {
    color: var(--faint);
    font-size: 11px;
    letter-spacing: -1px;
  }

  .chip.add {
    border-style: dashed;
    color: var(--muted);
    cursor: pointer;
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

    .box,
    .box.short {
      height: 44px;
      width: 100%;
    }

    .box.num {
      width: 120px;
    }
  }
</style>
