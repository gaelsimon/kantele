<script lang="ts">
  import {
    getConfiguration,
    getProgress,
    getStatus,
    type Configuration,
    type Progress,
    type Status,
  } from './lib/api';
  import { clock, count, uptime } from './lib/say';
  import Library from './lib/Library.svelte';
  import Settings from './lib/Settings.svelte';

  let status = $state<Status | null>(null);
  let progress = $state<Progress | null>(null);
  let configuration = $state<Configuration | null>(null);
  let unreachable = $state<string | null>(null);
  let tab = $state<'library' | 'settings'>('settings');

  const running = $derived(progress !== null && progress.phase !== 'idle');
  const failing = $derived(status?.failing);

  async function readStatus() {
    try {
      status = await getStatus();
      unreachable = null;
    } catch (error) {
      unreachable = error instanceof Error ? error.message : String(error);
    }
  }

  async function readProgress() {
    try {
      progress = await getProgress();
    } catch {
      // The status poll is what says the server is gone; this one stays quiet.
    }
  }

  async function readConfiguration() {
    try {
      configuration = await getConfiguration();
    } catch {
      configuration = null;
    }
  }

  // A NAS counts the whole library for a status, so a tab nobody is looking at asks for nothing.
  $effect(() => {
    let slow: ReturnType<typeof setInterval> | undefined;
    let quick: ReturnType<typeof setInterval> | undefined;

    function stop() {
      clearInterval(slow);
      clearInterval(quick);
      slow = quick = undefined;
    }

    function start() {
      if (slow !== undefined) return;
      void readStatus();
      void readProgress();
      slow = setInterval(readStatus, 5_000);
      quick = setInterval(readProgress, 1_000);
    }

    function visibility() {
      if (document.hidden) stop();
      else start();
    }

    void readConfiguration();
    visibility();
    document.addEventListener('visibilitychange', visibility);
    return () => {
      stop();
      document.removeEventListener('visibilitychange', visibility);
    };
  });

  /// What a pass underway has got through, in one line.
  function reading(underway: Progress): string {
    if (underway.phase === 'walking') {
      return `Listing the library: ${count(underway.folders)} folders so far.`;
    }
    if (underway.phase === 'building') return 'Building the menus.';
    const total = underway.found;
    return total > 0
      ? `Reading the library: ${count(underway.read)} of ${count(total)} files.`
      : 'Reading the library.';
  }

  const through = $derived(
    progress && progress.found > 0 ? Math.min(100, (progress.read / progress.found) * 100) : 0,
  );
</script>

<header>
  <div class="what">
    <span class="name">{status?.name ?? 'Kantele'}</span>
    <span class="dim version">{status?.version ?? ''}</span>
  </div>
  <nav>
    <button class:here={tab === 'settings'} onclick={() => (tab = 'settings')}>Settings</button>
    <button class:here={tab === 'library'} onclick={() => (tab = 'library')}>Library</button>
  </nav>
  <div class="dim up">{status ? `up ${uptime(status.uptime_seconds)}` : ''}</div>
</header>

{#if running && progress}
  <div class="strip">
    <div class="dim">{reading(progress)}</div>
    <div class="bar"><div class="through" style:width="{through}%"></div></div>
  </div>
{/if}

{#if unreachable}
  <div class="banner">
    <span class="dot"></span>
    <div>
      <strong>This page cannot reach the server.</strong>
      {unreachable} Is Kantele still running on this port?
    </div>
  </div>
{:else if failing}
  <div class="banner">
    <span class="dot"></span>
    <div>
      <strong>The library has not been read since {clock(failing.since)}.</strong>
      {failing.why}
    </div>
  </div>
{/if}

<main>
  {#if tab === 'settings'}
    <Settings {configuration} {status} onsaved={readConfiguration} />
  {:else}
    <Library
      {status}
      {configuration}
      onrescanned={readStatus}
      onsettings={() => (tab = 'settings')}
    />
  {/if}
</main>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 0 56px;
    height: 64px;
    background: var(--surface);
    border-bottom: 1px solid var(--edge);
  }

  .what {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  .name {
    font-size: 18px;
    font-weight: 600;
  }

  .version,
  .up {
    font-size: 13px;
  }

  nav {
    display: flex;
    gap: 32px;
    height: 64px;
  }

  nav button {
    display: flex;
    align-items: center;
    border-bottom: 3px solid transparent;
    color: var(--muted);
    font-size: 15px;
  }

  nav button.here {
    border-bottom-color: var(--ink);
    color: var(--ink);
    font-weight: 600;
  }

  .strip {
    background: var(--surface);
    border-bottom: 1px solid var(--edge);
    padding: 11px 56px;
    display: flex;
    align-items: center;
    gap: 20px;
    font-size: 13px;
  }

  .bar {
    width: 220px;
    height: 6px;
    background: var(--hairline);
    flex-shrink: 0;
  }

  .through {
    height: 6px;
    background: var(--ink);
    transition: width 0.3s linear;
  }

  .banner {
    background: var(--warn-paper);
    border-bottom: 1px solid var(--warn-edge);
    padding: 14px 56px;
    display: flex;
    align-items: baseline;
    gap: 14px;
    color: var(--warn-ink);
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--warn);
    flex-shrink: 0;
  }

  main {
    padding: 28px 56px;
  }

  @media (max-width: 720px) {
    header {
      flex-wrap: wrap;
      height: auto;
      padding: 14px 20px 0;
      gap: 0;
    }

    .what {
      flex: 1;
    }

    nav {
      order: 3;
      width: 100%;
      height: auto;
      gap: 24px;
    }

    nav button {
      padding: 10px 0;
    }

    .strip,
    .banner {
      padding-left: 20px;
      padding-right: 20px;
    }

    .strip {
      flex-direction: column;
      align-items: stretch;
      gap: 6px;
    }

    .bar {
      width: auto;
    }

    main {
      padding: 20px;
    }
  }
</style>
