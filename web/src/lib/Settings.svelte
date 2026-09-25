<script lang="ts">
  import {
    getLog,
    getMenu,
    writeConfiguration,
    type Configuration,
    type LogTail,
    type Menu,
    type Setting,
    type Status,
  } from './api';
  import { costClass, dearest, shared } from './apply';
  import { PLAYERS, arrange, shapeOf, type Section } from './settings';
  import { listed } from './say';
  import Field from './Field.svelte';
  import Screen from './Screen.svelte';

  let {
    configuration,
    status,
    onsaved,
  }: { configuration: Configuration | null; status: Status | null; onsaved: () => void } =
    $props();

  let draft = $state<Record<string, unknown>>({});
  let said = $state('');
  let failed = $state('');
  let saving = $state(false);
  let root = $state<Menu | null>(null);
  let log = $state<LogTail | null>(null);
  let logSaid = $state('');

  /// The last lines the server wrote, asked for when somebody wants them and not before.
  async function readLog() {
    try {
      log = await getLog();
      logSaid = '';
    } catch (error) {
      log = null;
      logSaid = error instanceof Error ? error.message : String(error);
    }
  }

  const settings = $derived(configuration?.settings ?? []);
  const sections = $derived(arrange(settings));
  const unsaved = $derived(Object.keys(draft));
  const pending = $derived(settings.filter((setting) => setting.key in draft));
  const cost = $derived(dearest(pending)?.says ?? '');
  // A first start, where the only useful thing to do is choose a folder.
  const folderless = $derived(
    configuration !== null && !settings.find((setting) => setting.key === 'content_dir')?.value,
  );

  function valueOf(setting: Setting): unknown {
    return setting.key in draft ? draft[setting.key] : setting.as_written;
  }

  /// The value in force or in the draft for a key, or nothing where the server has no such key.
  function current(key: string): unknown {
    const setting = settings.find((one) => one.key === key);
    return setting ? valueOf(setting) : undefined;
  }

  const screenName = $derived(String(current('friendly_name') ?? ''));
  const screenAxes = $derived.by(() => {
    const axes = settings.find((one) => one.key === 'menus.axes');
    const codes = current('menus.axes');
    if (!Array.isArray(codes)) return [];
    return (codes as string[]).map(
      (code) => axes?.choices?.find((choice) => choice.value === code)?.label ?? code,
    );
  });
  const screenRecent = $derived(Number(current('menus.recent') ?? 0));
  /// The axes the server was asked for and did not offer, because they narrow nothing. An axis
  /// added since is drawn, since nobody has asked the server about it yet.
  const hiddenAxes = $derived.by(() => {
    if (!root) return [];
    const axes = settings.find((one) => one.key === 'menus.axes');
    const asked = Array.isArray(axes?.as_written) ? (axes.as_written as string[]) : [];
    const offered = new Set(
      root.entries.filter((entry) => entry.chosen).map((entry) => entry.title),
    );
    return asked
      .map((code) => axes?.choices?.find((choice) => choice.value === code)?.label ?? code)
      .filter((label) => !offered.has(label));
  });
  const holds = $derived({
    albums: status?.library.albums ?? 0,
    tracks: status?.library.tracks ?? 0,
    untagged: status?.library.untagged ?? 0,
    playlists: status?.library.playlists ?? 0,
  });

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

  /// The root a player meets now, which says which axes narrow nothing. A failure leaves it
  /// unknown, and every axis is drawn.
  async function readRoot() {
    try {
      root = await getMenu();
    } catch {
      root = null;
    }
  }

  $effect(() => {
    if (configuration) void readRoot();
  });

  async function save() {
    saving = true;
    try {
      const answer = await writeConfiguration(draft);
      said = answer.says;
      failed = '';
      draft = {};
      onsaved();
      void readRoot();
    } catch (error) {
      failed = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }
</script>

{#snippet fields(section: Section)}
  {@const badge = section.each ? undefined : shared(section.settings)}
  {#each section.settings as setting (setting.key)}
    <Field
      {setting}
      shape={shapeOf(setting)}
      value={valueOf(setting)}
      changed={setting.key in draft}
      tagged={section.each || (badge !== undefined && setting.apply !== badge.apply)}
      onchange={(value) => edit(setting, value)}
    />
  {/each}
{/snippet}

<div class="flat">
  {#if folderless}
    <div class="first">Select the folder that has your music. Then click Save.</div>
  {/if}
  {#if failed}
    <div class="problem line">{failed}</div>
  {:else if said}
    <div class="dim line">{said}</div>
  {/if}

  {#each sections as section (section.title)}
    {@const badge = section.each ? undefined : shared(section.settings)}
    <section class="card sect">
      <div class="bhead">
        <div class="stitle">{section.title}</div>
        {#if badge}
          <span class="cost {costClass(badge.apply)}" title={badge.says}>{badge.tag}</span>
        {:else if section.each}
          <span class="dim each">each setting says when a change applies</span>
        {/if}
      </div>
      {#if section.title === PLAYERS}
        <div class="expose">
          <div>{@render fields(section)}</div>
          <aside class="amp">
            <div class="cap">Preview</div>
            <Screen
              name={screenName}
              axes={screenAxes}
              recent={screenRecent}
              {holds}
              hidden={hiddenAxes}
            />
            <div class="dim foot">The grey entries are always on the menu.</div>
            {#if hiddenAxes.length > 0}
              <div class="dim foot">
                {listed(hiddenAxes)} {hiddenAxes.length === 1 ? 'is' : 'are'} not on the menu. All
                tracks have the same value.
              </div>
            {/if}
          </aside>
        </div>
      {:else}
        {@render fields(section)}
      {/if}
      {#if section.note}
        <div class="dim foot">{section.note}</div>
      {/if}
    </section>
  {/each}

  <section class="card sect">
    <div class="bhead">
      <div class="stitle">What the server said</div>
      <button class="button" onclick={readLog}>{log ? 'Refresh' : 'Show the last lines'}</button>
    </div>
    {#if logSaid}
      <div class="dim foot">{logSaid}</div>
    {:else if log}
      <pre class="log">{log.lines.join('\n')}</pre>
      <div class="dim foot">The file is <span class="mono">{log.path}</span>.</div>
    {:else}
      <div class="dim foot">
        The end of the server's own log, which is where a start that went wrong says why.
      </div>
    {/if}
  </section>

  <div class="dim file">
    {#if configuration?.file}
      Settings file: <span class="mono">{configuration.file}</span>
    {:else if configuration}
      This server was started without a settings file, so nothing here can be saved. Start it with
      <span class="mono">--config</span>.
    {/if}
  </div>
</div>

{#if unsaved.length > 0}
  <div class="savebar">
    <button class="button strong" disabled={saving} onclick={save}>
      {saving ? 'Saving' : 'Save'}
    </button>
    <button class="button" disabled={saving} onclick={() => (draft = {})}>Discard</button>
    <span class="count">
      {unsaved.length === 1 ? '1 change not saved' : `${unsaved.length} changes not saved`}
    </span>
    <span class="dim what">{cost}</span>
  </div>
{/if}

<style>
  .flat {
    display: flex;
    flex-direction: column;
    gap: 24px;
    max-width: 980px;
    margin: 0 auto;
    padding-bottom: 72px;
  }

  .first {
    font-size: 16px;
    padding: 0 4px;
  }

  .line {
    font-size: 13px;
    padding: 0 4px;
  }

  .sect {
    display: flex;
    flex-direction: column;
  }

  .bhead {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    margin-bottom: 10px;
  }

  .stitle {
    font-size: 16px;
    font-weight: 600;
  }

  .each {
    font-size: 12px;
  }

  .expose {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 240px;
    gap: 20px;
    align-items: start;
  }

  .amp {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .log {
    margin: 0 0 8px;
    padding: 12px;
    max-height: 320px;
    overflow: auto;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.45;
    white-space: pre;
    background: var(--surface);
    border: 1px solid var(--edge);
    border-radius: 6px;
  }

  .foot {
    font-size: 12px;
    margin-top: 12px;
  }

  .file {
    font-size: 13px;
    padding: 0 4px;
    overflow-wrap: anywhere;
  }

  .savebar {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    background: var(--surface);
    border-top: 1px solid var(--edge);
    padding: 12px 56px;
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
    box-shadow: 0 -6px 18px rgba(0, 0, 0, 0.06);
  }

  .count {
    font-size: 13px;
    color: var(--link);
  }

  .what {
    font-size: 13px;
  }

  @media (max-width: 720px) {
    .flat {
      gap: 16px;
      padding-bottom: 110px;
    }

    .expose {
      grid-template-columns: minmax(0, 1fr);
    }

    .savebar {
      padding: 12px 20px;
    }

    .savebar .button {
      height: 44px;
    }
  }
</style>
