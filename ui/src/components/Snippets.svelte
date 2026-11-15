<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Trash2, Terminal } from "@lucide/svelte";
  import { api, type Connection, type Snippet } from "../ipc";
  import { connections, pushToast } from "../stores";
  import Modal from "./Modal.svelte";

  let {
    onClose,
  }: {
    onClose: () => void;
  } = $props();

  let snippets = $state<Snippet[]>([]);
  let name = $state("");
  let command = $state("");
  let scope = $state<string | null>(null);
  let busy = $state(false);

  async function refresh() {
    try {
      // The manage list shows everything (global + scoped).
      snippets = await api.listSnippets();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  onMount(refresh);

  async function save() {
    busy = true;
    try {
      await api.saveSnippet({
        id: null,
        name,
        command,
        connection_id: scope,
      });
      name = "";
      command = "";
      scope = null;
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    } finally {
      busy = false;
    }
  }

  async function remove(snippet: Snippet) {
    try {
      await api.deleteSnippet(snippet.id);
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  function connectionName(id: string | null): string {
    if (!id) return "global";
    const c: Connection | undefined = $connections.find((c) => c.id === id);
    return c?.name ?? "unknown";
  }
</script>

<Modal title="Snippets" width={560} {onClose}>
  <div class="body">
    <p class="hint">
      Snippets appear in Ctrl+K while a session is active; running one sends
      its command to the terminal. Global snippets are available everywhere;
      scoped ones only for their connection.
    </p>

    {#if snippets.length > 0}
      <div class="list">
        {#each snippets as snippet (snippet.id)}
          <div class="row">
            <div class="info">
              <span class="name">{snippet.name}</span>
              <span class="cmd mono">{snippet.command}</span>
              <span class="scope">{connectionName(snippet.connection_id)}</span>
            </div>
            <button class="icon-btn danger" title="Delete" onclick={() => remove(snippet)}>
              <Trash2 size={14} />
            </button>
          </div>
        {/each}
      </div>
    {:else}
      <p class="hint empty">No snippets yet.</p>
    {/if}

    <form
      class="editor"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <div class="row two">
        <div class="field">
          <label for="s-name">Name</label>
          <input id="s-name" bind:value={name} placeholder="restart nginx" required spellcheck="false" />
        </div>
        <div class="field">
          <label for="s-scope">Scope</label>
          <select id="s-scope" bind:value={scope}>
            <option value={null}>Global</option>
            {#each $connections as connection (connection.id)}
              <option value={connection.id}>{connection.name}</option>
            {/each}
          </select>
        </div>
      </div>
      <div class="field">
        <label for="s-command">Command</label>
        <input id="s-command" class="mono" bind:value={command} placeholder="sudo systemctl restart nginx" required spellcheck="false" />
      </div>
      <div class="actions">
        <button type="submit" class="btn primary" disabled={busy}>
          <Plus size={14} />
          {busy ? "Saving..." : "Add snippet"}
        </button>
      </div>
    </form>

    {#if snippets.length === 0}
      <p class="hint">
        <Terminal size={13} style="vertical-align: -2px" />
        Tip: snippets with multiple lines are sent line by line — the terminal
        runs them in order.
      </p>
    {/if}
  </div>
</Modal>

<style>
  .body {
    padding: 2px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 220px;
    overflow-y: auto;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .name {
    font-weight: 600;
  }

  .cmd {
    font-size: 11.5px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .scope {
    font-size: 11px;
    color: var(--text-faint);
  }

  .editor {
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .row.two {
    display: flex;
    gap: 10px;
  }

  .row.two .field {
    flex: 1;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field label {
    font-size: 11.5px;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }

  .empty {
    margin: 0;
  }
</style>
