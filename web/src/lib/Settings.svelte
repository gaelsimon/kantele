<script lang="ts">
  import { writeConfiguration, type Configuration, type Setting } from './api';
  import { badgeFor } from './apply';
  import { arrange, shapeOf } from './settings';
  import Field from './Field.svelte';

  let {
    configuration,
    onsaved,
  }: { configuration: Configuration | null; onsaved: () => void } = $props();

  let draft = $state<Record<string, unknown>>({});
  let said = $state('');
  let failed = $state('');
  let saving = $state(false);

  const settings = $derived(configuration?.settings ?? []);
  const unsaved = $derived(Object.keys(draft));

  const shown = $derived(arrange(settings));

  function valueOf(setting: Setting): unknown {
    return setting.key in draft ? draft[setting.key] : setting.as_written;
  }

  function edit(setting: Setting, value: unknown) {
    const { [setting.key]: _dropped, ...rest } = draft;
    // A field put back the way it was is not a change to save.
    draft =
      JSON.stringify(value) === JSON.stringify(setting.as_written)
        ? rest
        : { ...rest, [setting.key]: value };
    said = '';
    failed = '';
  }

  async function save() {
    saving = true;
    try {
      const answer = await writeConfiguration(draft);
      said = answer.says;
      failed = '';
      draft = {};
      onsaved();
    } catch (error) {
      failed = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }
</script>

<div class="top">
  <div class="title">Settings</div>
  <div class="dim aside">
    {#if configuration?.file}
      Saved to {configuration.file}. Greyed values come from the environment or the command line.
    {:else}
      This server was started without a settings file, so nothing here can be written. Start it with
      <span class="mono">--config</span>.
    {/if}
  </div>
</div>

<div class="columns">
  {#each [0, 1] as side (side)}
    <div class="column">
      {#each shown.filter((block) => block.side === side) as block (block.title)}
        <section class="card block">
          <div class="head">
            <div class="cap">{block.title}</div>
            <div class="dim badge">{badgeFor(block.settings)}</div>
          </div>
          {#each block.settings as setting (setting.key)}
            <Field
              {setting}
              shape={shapeOf(setting)}
              value={valueOf(setting)}
              changed={setting.key in draft}
              onchange={(value) => edit(setting, value)}
            />
          {/each}
          {#if block.note}
            <div class="dim foot">{block.note}</div>
          {/if}
        </section>
      {/each}
    </div>
  {/each}
</div>

<div class="save">
  <button class="button strong" disabled={unsaved.length === 0 || saving} onclick={save}>
    {saving ? 'Saving' : 'Save'}
  </button>
  <button class="button" disabled={unsaved.length === 0 || saving} onclick={() => (draft = {})}>
    Discard
  </button>
  {#if unsaved.length > 0}
    <span class="count">
      {unsaved.length === 1 ? 'one unsaved change' : `${unsaved.length} unsaved changes`}
    </span>
  {/if}
  {#if failed}<span class="problem">{failed}</span>{:else if said}<span class="dim">{said}</span>{/if}
</div>

<style>
  .top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 24px;
    margin-bottom: 24px;
    flex-wrap: wrap;
  }

  .title {
    font-size: 16px;
    font-weight: 600;
  }

  .aside {
    font-size: 13px;
    overflow-wrap: anywhere;
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 24px;
    align-items: start;
  }

  .column {
    display: flex;
    flex-direction: column;
    gap: 24px;
    min-width: 0;
  }

  .block {
    display: flex;
    flex-direction: column;
    padding: 22px 26px;
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 16px;
    margin-bottom: 6px;
  }

  .badge {
    font-size: 12px;
  }

  .foot {
    font-size: 12px;
    margin-top: 10px;
  }

  .save {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 24px;
    flex-wrap: wrap;
  }

  .button.strong {
    height: 40px;
    padding: 0 22px;
    background: var(--ink);
    color: var(--surface);
    border-color: var(--ink);
  }

  .button.strong:hover:not(:disabled) {
    background: #1f2226;
  }

  .button.strong:disabled {
    background: var(--hairline);
    border-color: var(--hairline);
    color: var(--faint);
  }

  .count {
    font-size: 13px;
    color: var(--link);
  }

  .problem,
  .save .dim {
    font-size: 13px;
  }

  @media (max-width: 720px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
      gap: 16px;
    }

    .column {
      gap: 16px;
    }

    .save {
      position: sticky;
      bottom: 0;
      background: var(--paper);
      padding: 12px 0;
      margin-top: 16px;
    }

    .save .button {
      height: 44px;
    }
  }
</style>
