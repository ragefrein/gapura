<script lang="ts">
  import {
    Plus,
    Search,
    Settings,
    KeyRound,
    FileUp,
    Lock,
    FolderClosed,
    Terminal,
    Pencil,
    Trash2,
  } from "@lucide/svelte";
  import type { Connection } from "../ipc";
  import { connections, groups, activeView, sessions, type SessionTab } from "../stores";
  import { fuzzyFilter } from "../fuzzy";

  let {
    onConnect,
    onEdit,
    onDelete,
    onNewConnection,
    onImport,
    onLock,
  }: {
    search: string;
    onConnect: (connection: Connection) => void;
    onEdit: (connection: Connection) => void;
    onDelete: (connection: Connection) => void;
    onNewConnection: () => void;
    onImport: () => void;
    onLock: () => void;
  } = $props();

  let query = $state("");
  let collapsed = $state<Set<string>>(new Set());
  let searchInput: HTMLInputElement | undefined = $state();

  function toggleGroup(id: string) {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    collapsed = next;
  }

  const visibleConnections = $derived(
    query.trim()
      ? fuzzyFilter($connections, (c) => `${c.name} ${c.host} ${c.username} ${c.tags.join(" ")}`, query)
      : $connections,
  );

  function connectionsOf(groupId: string | null): Connection[] {
    return visibleConnections.filter((c) => c.group_id === groupId);
  }

  const visibleGroups = $derived(
    query.trim()
      ? $groups.filter((g) => visibleConnections.some((c) => c.group_id === g.id))
      : $groups,
  );

  const ungrouped = $derived(connectionsOf(null));

  function sessionFor(connection: Connection): SessionTab | undefined {
    return $sessions.find((s) => s.connectionId === connection.id);
  }

  export function focusSearch() {
    searchInput?.focus();
  }
</script>

<aside class="sidebar">
  <div class="top">
    <div class="brand">
      <Terminal size={16} />
      <span>Gapura</span>
      <button
        class="icon-btn lock"
        title="Lock vault"
        onclick={onLock}
      >
        <Lock size={14} />
      </button>
    </div>
    <div class="search">
      <Search size={14} />
      <input
        bind:this={searchInput}
        bind:value={query}
        placeholder="Search connections"
        spellcheck="false"
      />
    </div>
  </div>

  <nav class="list">
    {#each visibleGroups as group (group.id)}
      {@const items = connectionsOf(group.id)}
      <button class="group-row" onclick={() => toggleGroup(group.id)}>
        {#if collapsed.has(group.id)}
          <FolderClosed size={14} />
        {:else}
          <FolderClosed size={14} class="open" />
        {/if}
        <span class="group-name">{group.name}</span>
        <span class="count">{items.length}</span>
      </button>
      {#if !collapsed.has(group.id)}
        {#each items as connection (connection.id)}
          {@render connectionRow(connection)}
        {/each}
      {/if}
    {/each}

    {#if ungrouped.length || visibleGroups.length === 0}
      {#if visibleGroups.length > 0}
        <div class="group-row plain"><span class="group-name">Other</span></div>
      {/if}
      {#each ungrouped as connection (connection.id)}
        {@render connectionRow(connection)}
      {/each}
    {/if}

    {#if visibleConnections.length === 0}
      <p class="empty">No connections match.</p>
    {/if}
  </nav>

  <footer>
    <button class="new" onclick={onNewConnection}>
      <Plus size={15} />
      <span>New connection</span>
    </button>
    <div class="tools">
      <button class="icon-btn" title="Import ~/.ssh/config" onclick={onImport}>
        <FileUp size={16} />
      </button>
      <button
        class="icon-btn"
        title="Keys"
        onclick={() => activeView.set("keys")}
      >
        <KeyRound size={16} />
      </button>
      <button
        class="icon-btn"
        title="Settings"
        onclick={() => activeView.set("settings")}
      >
        <Settings size={16} />
      </button>
    </div>
  </footer>
</aside>

{#snippet connectionRow(connection: Connection)}
  {@const live = sessionFor(connection)}
  <div
    class="connection"
    class:active={$sessions.some((s) => s.connectionId === connection.id && s.state !== "closed")}
    role="button"
    tabindex="0"
    ondblclick={() => onConnect(connection)}
    onkeydown={(e) => {
      if (e.key === "Enter") onConnect(connection);
    }}
  >
    <span
      class="dot"
      style={connection.color ? `background: ${connection.color}` : ""}
      class:plain={!connection.color}
    ></span>
    <span class="name" title={`${connection.username}@${connection.host}:${connection.port}`}>
      {connection.name}
    </span>
    {#if live}
      <span
        class="live-dot"
        class:closed={live.state === "closed"}
        class:connecting={live.state === "connecting"}
        title={live.state === "closed" ? live.closedReason : live.state}
      ></span>
    {/if}
    <span class="actions">
      <button
        class="icon-btn"
        title="Edit"
        onclick={(e) => {
          e.stopPropagation();
          onEdit(connection);
        }}
      >
        <Pencil size={13} />
      </button>
      <button
        class="icon-btn danger"
        title="Delete"
        onclick={(e) => {
          e.stopPropagation();
          onDelete(connection);
        }}
      >
        <Trash2 size={13} />
      </button>
    </span>
  </div>
{/snippet}

<style>
  .sidebar {
    width: 264px;
    min-width: 264px;
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border-right: 1px solid var(--border);
  }

  .top {
    padding: 12px 12px 8px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 650;
    letter-spacing: 0.2px;
    color: var(--accent);
  }

  .brand span {
    flex: 1;
  }

  .lock {
    width: 24px;
    height: 24px;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 7px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0 9px;
    color: var(--text-faint);
  }

  .search input {
    flex: 1;
    border: none;
    background: transparent;
    padding: 7px 0;
  }

  .search input:focus-visible {
    outline: none;
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }

  .group-row {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 7px 8px;
    color: var(--text-muted);
    border-radius: 6px;
    font-size: 12px;
  }

  .group-row:hover {
    background: var(--bg-hover);
  }

  .group-row.plain {
    cursor: default;
    margin-top: 6px;
  }

  .group-row.plain:hover {
    background: none;
  }

  .group-row :global(.open) {
    color: var(--accent);
  }

  .group-name {
    flex: 1;
    text-align: left;
    font-weight: 600;
  }

  .count {
    font-size: 11px;
    color: var(--text-faint);
  }

  .connection {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 6px 26px;
    border-radius: 6px;
    cursor: pointer;
    color: var(--text);
  }

  .connection:hover,
  .connection:focus-visible {
    background: var(--bg-hover);
    outline: none;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }

  .dot.plain {
    background: var(--text-faint);
    opacity: 0.55;
  }

  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .live-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    flex: none;
  }

  .live-dot.connecting {
    background: var(--warning);
    animation: pulse 1s infinite alternate;
  }

  .live-dot.closed {
    background: var(--danger);
  }

  @keyframes pulse {
    from { opacity: 0.4; }
    to { opacity: 1; }
  }

  .actions {
    display: none;
    align-items: center;
    gap: 2px;
  }

  .connection:hover .actions,
  .connection:focus-within .actions {
    display: inline-flex;
  }

  .actions .icon-btn {
    width: 22px;
    height: 22px;
  }

  .empty {
    color: var(--text-faint);
    padding: 12px 10px;
  }

  footer {
    border-top: 1px solid var(--border);
    padding: 10px 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .new {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 10px;
    border-radius: var(--radius);
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }

  .new:hover {
    filter: brightness(1.15);
  }

  .tools {
    display: flex;
    gap: 2px;
  }
</style>
