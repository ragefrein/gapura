<script lang="ts">
  import {
    KeyRound,
    Plus,
    Search,
    Settings,
    FileUp,
    Lock,
    Sun,
    SquareTerminal as TerminalSquareIcon,
  } from "@lucide/svelte";
  import { api, type Connection, type Snippet } from "../ipc";
  import { activeSessionId, activeView, connections, pushToast, sessions } from "../stores";
  import { utf8ToBase64 } from "../terminalBus";
  import { fuzzyFilter } from "../fuzzy";

  let {
    onConnect,
    onQuickConnect,
    onManageSnippets,
    onClose,
  }: {
    onConnect: (connection: Connection) => void;
    onQuickConnect: (text: string) => void;
    onManageSnippets: () => void;
    onClose: () => void;
  } = $props();

  interface Action {
    id: string;
    label: string;
    icon: typeof Plus;
    run: () => void | Promise<void>;
  }

  let query = $state("");
  let selected = $state(0);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    input?.focus();
  });

  const actions: Action[] = [
    {
      id: "action-new",
      label: "New connection",
      icon: Plus,
      run: () => {
        activeView.set("terminal");
        window.dispatchEvent(new CustomEvent("gapura:new-connection"));
      },
    },
    {
      id: "action-import",
      label: "Import ~/.ssh/config",
      icon: FileUp,
      run: () => {
        window.dispatchEvent(new CustomEvent("gapura:import"));
      },
    },
    {
      id: "action-keys",
      label: "Manage keys",
      icon: KeyRound,
      run: () => activeView.set("keys"),
    },
    {
      id: "action-snippets",
      label: "Manage snippets",
      icon: TerminalSquareIcon,
      run: () => onManageSnippets(),
    },
    {
      id: "action-settings",
      label: "Settings",
      icon: Settings,
      run: () => activeView.set("settings"),
    },
    {
      id: "action-theme",
      label: "Toggle theme",
      icon: Sun,
      run: () => window.dispatchEvent(new CustomEvent("gapura:toggle-theme")),
    },
    {
      id: "action-lock",
      label: "Lock vault",
      icon: Lock,
      run: () => api.vaultLock().then(() => window.location.reload()),
    },
  ];

  // Typing an ssh command offers one-click connection creation.
  const quickConnectAction = $derived.by(() => {
    const q = query.trim();
    if (q.length < 5 || !q.toLowerCase().startsWith("ssh")) return null;
    return {
      id: "quick-connect",
      label: `Create connection from: ${q}`,
      icon: Plus,
      run: () => onQuickConnect(q),
    };
  });

  const matchedConnections = $derived(
    query.trim()
      ? fuzzyFilter($connections, (c) => `${c.name} ${c.host} ${c.username}`, query)
      : $connections,
  );

  const matchedActions = $derived(
    query.trim() ? fuzzyFilter(actions, (a) => a.label, query) : actions,
  );

  // Snippets for the active session's connection; running one types it into
  // the terminal and presses enter.
  const activeSession = $derived($sessions.find((s) => s.id === $activeSessionId && s.state !== "closed") ?? null);
  let snippets = $state<Snippet[]>([]);
  $effect(() => {
    if (activeSession) {
      void api.listSnippets(activeSession.connectionId).then((list) => (snippets = list));
    }
  });
  const matchedSnippets = $derived.by(() => {
    if (!activeSession) return [];
    if (!query.trim()) return snippets;
    return fuzzyFilter(snippets, (s) => `${s.name} ${s.command}`, query);
  });

  const flatResults = $derived.by(() => {
    const results: Array<
      | { type: "connection"; value: Connection }
      | { type: "action"; value: Action }
      | { type: "snippet"; value: Snippet }
    > = [];
    if (quickConnectAction) results.push({ type: "action", value: quickConnectAction });
    for (const c of matchedConnections) results.push({ type: "connection", value: c });
    for (const sn of matchedSnippets) results.push({ type: "snippet", value: sn });
    for (const a of matchedActions) results.push({ type: "action", value: a });
    return results;
  });

  const snippetBase = $derived((quickConnectAction ? 1 : 0) + matchedConnections.length);
  const actionBase = $derived(snippetBase + matchedSnippets.length);

  $effect(() => {
    if (selected >= flatResults.length) selected = Math.max(0, flatResults.length - 1);
  });

  function move(delta: number) {
    if (flatResults.length === 0) return;
    selected = (selected + delta + flatResults.length) % flatResults.length;
  }

  function runSelected() {
    const entry = flatResults[selected];
    if (!entry) return;
    if (entry.type === "connection") onConnect(entry.value);
    else if (entry.type === "snippet") {
      const active = $sessions.find((s) => s.id === $activeSessionId);
      if (active) {
        void api.sessionWrite(active.id, utf8ToBase64(entry.value.command + "\n"));
        pushToast("info", `Sent: ${entry.value.name}`);
      }
      onClose();
    } else void entry.value.run();
    onClose();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      onClose();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      runSelected();
    }
  }
</script>

<svelte:window on:keydown={onKeydown} />

<div
  class="overlay"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
    <div class="search">
      <Search size={15} />
      <input
        bind:this={input}
        bind:value={query}
        placeholder="Connect to a server or run a command"
        oninput={() => (selected = 0)}
        spellcheck="false"
      />
      <span class="kbd">esc</span>
    </div>

    <div class="results">
      {#if quickConnectAction}
        <button
          class="result"
          class:selected={selected === 0}
          onclick={() => {
            void quickConnectAction.run();
            onClose();
          }}
          onmousemove={() => (selected = 0)}
        >
          <span class="icon"><quickConnectAction.icon size={14} /></span>
          <span class="label">{quickConnectAction.label}</span>
        </button>
      {/if}
      {#if matchedConnections.length}
        <p class="section">Connections</p>
        {#each matchedConnections as connection, i (connection.id)}
          {@const index = i}
          <button
            class="result"
            class:selected={index === selected}
            onclick={() => {
              onConnect(connection);
              onClose();
            }}
            onmousemove={() => (selected = index)}
          >
            <span class="dot" style={connection.color ? `background: ${connection.color}` : ""}></span>
            <span class="label">{connection.name}</span>
            <span class="meta mono">{connection.username}@{connection.host}</span>
          </button>
        {/each}
      {/if}

      {#if matchedSnippets.length}
        <p class="section">Snippets</p>
        {#each matchedSnippets as snippet, k (snippet.id)}
          {@const index = snippetBase + k}
          <button
            class="result"
            class:selected={index === selected}
            onclick={() => {
              const active = $sessions.find((s) => s.id === $activeSessionId);
              if (active) {
                void api.sessionWrite(active.id, utf8ToBase64(snippet.command + "\n"));
                pushToast("info", `Sent: ${snippet.name}`);
              }
              onClose();
            }}
            onmousemove={() => (selected = index)}
          >
            <span class="icon"><TerminalSquareIcon size={14} /></span>
            <span class="label">{snippet.name}</span>
            <span class="meta mono">{snippet.command}</span>
          </button>
        {/each}
      {/if}

      {#if matchedActions.length}
        <p class="section">Actions</p>
        {#each matchedActions as action, j (action.id)}
          {@const index = actionBase + j}
          <button
            class="result"
            class:selected={index === selected}
            onclick={() => {
              void action.run();
              onClose();
            }}
            onmousemove={() => (selected = index)}
          >
            <span class="icon"><action.icon size={14} /></span>
            <span class="label">{action.label}</span>
          </button>
        {/each}
      {/if}

      {#if flatResults.length === 0}
        <p class="none">Nothing matches "{query}".</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(6, 9, 13, 0.5);
    z-index: 70;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
  }

  .palette {
    width: 560px;
    max-height: 62vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
    color: var(--text-faint);
  }

  .search input {
    flex: 1;
    border: none;
    background: transparent;
    padding: 0;
    font-size: 14px;
  }

  .search input:focus-visible {
    outline: none;
  }

  .results {
    overflow-y: auto;
    padding: 6px 6px 10px;
  }

  .section {
    margin: 8px 10px 2px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-faint);
  }

  .result {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 6px;
    text-align: left;
    color: var(--text);
  }

  .result.selected {
    background: var(--accent-soft);
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--text-faint);
    flex: none;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 16px;
    color: var(--text-muted);
  }

  .label {
    flex: 1;
  }

  .meta {
    font-size: 11px;
    color: var(--text-faint);
  }

  .none {
    color: var(--text-faint);
    padding: 14px;
  }
</style>
