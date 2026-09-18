<script lang="ts">
  import { screen, type Holdings } from './screen';

  let {
    name,
    axes,
    recent,
    holds,
    hidden = [],
  }: {
    name: string;
    axes: string[];
    recent: number;
    holds: Holdings;
    hidden?: readonly string[];
  } = $props();

  const drawn = $derived(screen(name, axes, recent, holds, hidden));
</script>

<!-- A device screen: dark whatever the page's theme, because it depicts a screen and not the page. -->
<div class="screen">
  <div class="crumb">Music servers</div>
  <div class="top">{drawn.name}</div>
  <ul>
    {#each drawn.rows as row (row.label)}
      <li class:fixed={row.fixed}>{row.label}</li>
    {/each}
  </ul>
</div>

<style>
  .screen {
    background: #23262a;
    color: #e8e6df;
    padding: 14px 16px;
    font-size: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .crumb {
    font-size: 11px;
    letter-spacing: 0.04em;
    color: #7d8189;
  }

  .top {
    font-weight: 600;
    padding-bottom: 6px;
    border-bottom: 1px solid #4a4f56;
    margin-bottom: 4px;
    overflow-wrap: anywhere;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }

  li {
    display: flex;
    justify-content: space-between;
    padding: 5px 0;
    color: #d8d6cf;
  }

  li::after {
    content: '›';
    color: #7d8189;
  }

  li.fixed {
    color: #8a8e96;
  }
</style>
