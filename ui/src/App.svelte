<script lang="ts">
  import { onMount } from "svelte";
  import { SquareTerminal } from "@lucide/svelte";
  import { api, isIpcError, type Connection } from "./ipc";
  import {
    activeView,
    connections,
    groups,
    pushToast,
    settings,
    vaultStatus,
  } from "./stores";
  import Sidebar from "./components/Sidebar.svelte";
  import ConnectionForm from "./components/ConnectionForm.svelte";
  import CommandPalette from "./components/CommandPalette.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import Toasts from "./components/Toasts.svelte";

  let paletteOpen = $state(false);
  let formTarget = $state<{ open: boolean; connection: Connection | null }>({
    open: false,
    connection: null,
  });
  let confirmDelete = $state<Connection | null>(null);

  function applyTheme(theme: "dark" | "light") {
    document.documentElement.dataset.theme = theme;
  }

  async function refreshConnections() {
    try {
      connections.set(await api.listConnections());
      groups.set(await api.listGroups());
    } catch (e) {
      pushToast("error", describe(e));
    }
  }

  function describe(error: unknown): string {
    if (isIpcError(error)) return error.message;
    if (error instanceof Error) return error.message;
    return String(error);
  }

  async function saveFromForm(saved: Connection) {
    formTarget = { open: false, connection: null };
    await refreshConnections();
    pushToast("info", `Saved "${saved.name}"`);
  }

  async function deleteConfirmed() {
    const target = confirmDelete;
    if (!target) return;
    confirmDelete = null;
    try {
      await api.deleteConnection(target.id);
      await refreshConnections();
      pushToast("info", `Deleted "${target.name}"`);
    } catch (e) {
      pushToast("error", describe(e));
    }
  }

  function toggleTheme() {
    const next = $settings.theme === "dark" ? "light" : "dark";
    settings.update((s) => ({ ...s, theme: next }));
    applyTheme(next);
    void api.saveSettings($settings);
  }

  onMount(() => {
    void (async () => {
      try {
        const loaded = await api.getSettings();
        settings.set(loaded);
        applyTheme(loaded.theme);
      } catch {
        applyTheme("dark");
      }
      vaultStatus.set(await api.vaultStatus());
      if ($vaultStatus.unlocked) await refreshConnections();
    })();

    // Cross-component actions (palette, future views).
    const onNew = () => (formTarget = { open: true, connection: null });
    const onToggleTheme = () => toggleTheme();
    window.addEventListener("gapura:new-connection", onNew);
    window.addEventListener("gapura:toggle-theme", onToggleTheme);

    const globalKeys = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        paletteOpen = !paletteOpen;
      }
    };
    window.addEventListener("keydown", globalKeys);

    return () => {
      window.removeEventListener("gapura:new-connection", onNew);
      window.removeEventListener("gapura:toggle-theme", onToggleTheme);
      window.removeEventListener("keydown", globalKeys);
    };
  });
</script>

<div class="app">
  <Sidebar
    search={""}
    onConnect={(connection) => {
      // Sessions land with the terminal work; editing stays available.
      formTarget = { open: true, connection };
    }}
    onEdit={(connection) => (formTarget = { open: true, connection })}
    onDelete={(connection) => (confirmDelete = connection)}
    onNewConnection={() => (formTarget = { open: true, connection: null })}
    onImport={() => pushToast("info", "Import lands with the onboarding update")}
    onLock={() => {
      void api.vaultLock().then(() => window.location.reload());
    }}
  />

  <main>
    {#if $activeView === "terminal"}
      <div class="empty-state">
        <SquareTerminal size={40} strokeWidth={1.4} />
        <p>Select a connection to start a session.</p>
        <p class="hint">Press <span class="kbd">Ctrl K</span> to search everything.</p>
      </div>
    {:else}
      <div class="empty-state">
        <p>This screen arrives with the next update.</p>
      </div>
    {/if}
  </main>
</div>

{#if paletteOpen}
  <CommandPalette
    onConnect={(connection) => (formTarget = { open: true, connection })}
    onClose={() => (paletteOpen = false)}
  />
{/if}

{#if formTarget.open}
  <ConnectionForm
    connection={formTarget.connection}
    onClose={() => (formTarget = { open: false, connection: null })}
    onSaved={saveFromForm}
  />
{/if}

{#if confirmDelete}
  <ConfirmDialog
    title="Delete connection"
    message={`Delete "${confirmDelete.name}" (${confirmDelete.username}@${confirmDelete.host})? The stored password is removed with it.`}
    onConfirm={deleteConfirmed}
    onCancel={() => (confirmDelete = null)}
  />
{/if}

<Toasts />

<style>
  .app {
    display: flex;
    height: 100vh;
  }

  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--bg);
  }

  .empty-state {
    flex: 1;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 10px;
    color: var(--text-faint);
    text-align: center;
    padding: 24px;
  }

  .empty-state p {
    margin: 0;
    color: var(--text-muted);
  }
</style>
