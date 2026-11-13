<script lang="ts">
  import { onMount } from "svelte";
  import { SquareTerminal } from "@lucide/svelte";
  import { api, isIpcError, listen, type Connection, type Theme } from "./ipc";
  import {
    activeSessionId,
    activeView,
    connections,
    groups,
    markSessionState,
    pushToast,
    sessions,
    settings,
    vaultStatus,
    type SessionTab,
  } from "./stores";
  import { writeToTerminal } from "./terminalBus";
  import Sidebar from "./components/Sidebar.svelte";
  import ConnectionForm from "./components/ConnectionForm.svelte";
  import CommandPalette from "./components/CommandPalette.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import SessionTabs from "./components/SessionTabs.svelte";
  import TerminalView from "./components/TerminalView.svelte";
  import HostKeyModal from "./components/HostKeyModal.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import UnlockScreen from "./components/UnlockScreen.svelte";
  import SettingsView from "./components/SettingsView.svelte";
  import KeysView from "./components/KeysView.svelte";
  import ImportReview from "./components/ImportReview.svelte";
  import Toasts from "./components/Toasts.svelte";

  interface HostKeyMismatch {
    host: string;
    port: number;
    storedFingerprint: string;
    actualFingerprint: string;
    connectionId: string;
  }

  let paletteOpen = $state(false);
  let formTarget = $state<{ open: boolean; connection: Connection | null }>({
    open: false,
    connection: null,
  });
  let confirmDelete = $state<Connection | null>(null);
  let hostKeyMismatch = $state<HostKeyMismatch | null>(null);
  let importOpen = $state(false);
  let bootstrapped = $state(false);

  /** Full-screen states: first-run wizard and the locked vault. */
  let stage = $derived.by(() => {
    if (!bootstrapped) return "boot";
    if (!$vaultStatus.exists) return "onboarding";
    if (!$vaultStatus.unlocked) return "locked";
    return "main";
  });

  function applyTheme(theme: Theme) {
    document.documentElement.dataset.theme = theme;
  }

  function describe(error: unknown): string {
    if (isIpcError(error)) return error.message;
    if (error instanceof Error) return error.message;
    return String(error);
  }

  async function refreshConnections() {
    try {
      connections.set(await api.listConnections());
      groups.set(await api.listGroups());
    } catch (e) {
      pushToast("error", describe(e));
    }
  }

  async function enterMain() {
    vaultStatus.set(await api.vaultStatus());
    if ($vaultStatus.unlocked) await refreshConnections();
  }

  // ---- session lifecycle ---------------------------------------------------

  async function connect(connection: Connection) {
    const existing = $sessions.find(
      (s) => s.connectionId === connection.id && s.state !== "closed",
    );
    if (existing) {
      activeSessionId.set(existing.id);
      activeView.set("terminal");
      return;
    }
    activeView.set("terminal");
    try {
      const info = await api.sessionOpen(connection.id, 80, 24);
      const tab: SessionTab = {
        id: info.id,
        connectionId: connection.id,
        name: connection.name,
        color: connection.color,
        state: "connected",
      };
      sessions.update((list) => [...list, tab]);
      activeSessionId.set(info.id);
      if (info.new_host_key) {
        pushToast(
          "info",
          `Host key pinned for ${connection.host} (${info.fingerprint.slice(0, 17)}...)`,
        );
      }
    } catch (e) {
      if (isIpcError(e) && e.code === "ssh.host_key_mismatch") {
        hostKeyMismatch = {
          host: e.host ?? connection.host,
          port: e.port ?? connection.port,
          storedFingerprint: e.stored_fingerprint ?? "",
          actualFingerprint: e.actual_fingerprint ?? "",
          connectionId: connection.id,
        };
        return;
      }
      pushToast("error", describe(e));
    }
  }

  async function reconnect(sessionId: string) {
    const session = $sessions.find((s) => s.id === sessionId);
    if (!session) return;
    try {
      markSessionState(sessionId, "connecting");
      const info = await api.sessionReconnect(sessionId, 80, 24);
      markSessionState(info.id, "connected");
      pushToast("info", `Reconnected to ${session.name}`);
    } catch (e) {
      markSessionState(sessionId, "closed", describe(e));
      pushToast("error", describe(e));
    }
  }

  async function closeSession(sessionId: string) {
    try {
      await api.sessionClose(sessionId);
    } catch {
      // Already gone on the backend; drop the tab regardless.
    }
    sessions.update((list) => list.filter((s) => s.id !== sessionId));
    const remaining = $sessions.filter((s) => s.id !== sessionId);
    if ($activeSessionId === sessionId) {
      activeSessionId.set(remaining[remaining.length - 1]?.id ?? null);
    }
  }

  async function acceptNewHostKey() {
    const mismatch = hostKeyMismatch;
    if (!mismatch) return;
    hostKeyMismatch = null;
    try {
      await api.removeKnownHost(`${mismatch.host}:${mismatch.port}`);
      const connection = $connections.find((c) => c.id === mismatch.connectionId);
      if (connection) await connect(connection);
    } catch (e) {
      pushToast("error", describe(e));
    }
  }

  // ---- misc actions --------------------------------------------------------

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
      for (const session of $sessions.filter(
        (s) => s.connectionId === target.id && s.state !== "closed",
      )) {
        await closeSession(session.id);
      }
      await api.deleteConnection(target.id);
      await refreshConnections();
      pushToast("info", `Deleted "${target.name}"`);
    } catch (e) {
      pushToast("error", describe(e));
    }
  }

  function setTheme(theme: Theme, persist: boolean) {
    settings.update((s) => ({ ...s, theme }));
    applyTheme(theme);
    if (persist) void api.saveSettings($settings);
  }

  function toggleTheme() {
    setTheme($settings.theme === "dark" ? "light" : "dark", true);
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];

    void listen<{ id: string; data: string }>("session:data", (event) => {
      const bytes = Uint8Array.from(atob(event.payload.data), (ch) => ch.charCodeAt(0));
      writeToTerminal(event.payload.id, bytes);
    }).then((off) => unlisteners.push(off));

    void listen<{ id: string; state: "connected" | { closed: string } }>(
      "session:status",
      (event) => {
        const state = event.payload.state;
        if (state === "connected") {
          markSessionState(event.payload.id, "connected");
        } else {
          markSessionState(event.payload.id, "closed", state.closed);
          pushToast("info", `Session closed: ${state.closed}`);
        }
      },
    ).then((off) => unlisteners.push(off));

    void listen<{ host: string; fingerprint: string }>("hostkey:added", (event) => {
      pushToast("info", `Host key pinned for ${event.payload.host}`);
    }).then((off) => unlisteners.push(off));

    const onNew = () => (formTarget = { open: true, connection: null });
    const onToggleTheme = () => toggleTheme();
    const onCloseSession = (event: Event) =>
      void closeSession((event as CustomEvent<string>).detail);
    const onUnlocked = () => void enterMain();
    const onOnboarded = () => void enterMain();
    window.addEventListener("gapura:new-connection", onNew);
    window.addEventListener("gapura:toggle-theme", onToggleTheme);
    window.addEventListener("gapura:close-session", onCloseSession);
    window.addEventListener("gapura:unlocked", onUnlocked);
    window.addEventListener("gapura:onboarded", onOnboarded);

    const globalKeys = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (stage === "main") paletteOpen = !paletteOpen;
      }
    };
    window.addEventListener("keydown", globalKeys);

    void (async () => {
      try {
        const loaded = await api.getSettings();
        settings.set(loaded);
        applyTheme(loaded.theme);
      } catch {
        applyTheme("dark");
      }
      vaultStatus.set(await api.vaultStatus());

      // Keychain mode unlocks silently (design doc 2.3: no passphrase per
      // launch); passphrase mode waits for the unlock screen.
      if ($vaultStatus.exists && $vaultStatus.mode === "keychain" && !$vaultStatus.unlocked) {
        try {
          await api.vaultUnlock();
          vaultStatus.set(await api.vaultStatus());
        } catch (e) {
          if (isIpcError(e) && e.code === "vault.keychain") {
            vaultStatus.update((v) => ({ ...v, keychain_error: e.message }));
          }
        }
      }
      if ($vaultStatus.unlocked) await refreshConnections();
    })().finally(() => {
      // Never wedge on the boot screen; a failed status check still shows
      // the onboarding path instead of a blank window.
      bootstrapped = true;
    });

    return () => {
      for (const off of unlisteners.splice(0)) off();
      window.removeEventListener("gapura:new-connection", onNew);
      window.removeEventListener("gapura:toggle-theme", onToggleTheme);
      window.removeEventListener("gapura:close-session", onCloseSession);
      window.removeEventListener("gapura:unlocked", onUnlocked);
      window.removeEventListener("gapura:onboarded", onOnboarded);
      window.removeEventListener("keydown", globalKeys);
    };
  });
</script>

{#if stage === "boot"}
  <div class="boot"></div>
{:else if stage === "onboarding"}
  <Onboarding />
{:else if stage === "locked"}
  <UnlockScreen />
{:else}
  <div class="app">
    <Sidebar
      onConnect={(connection) => {
        activeView.set("terminal");
        void connect(connection);
      }}
      onEdit={(connection) => (formTarget = { open: true, connection })}
      onDelete={(connection) => (confirmDelete = connection)}
      onNewConnection={() => (formTarget = { open: true, connection: null })}
      onImport={() => (importOpen = true)}
      onLock={() => {
        void api.vaultLock().then(() => window.location.reload());
      }}
    />

    <main>
      <SessionTabs />
      {#if $activeView === "terminal"}
        {#if $sessions.length > 0}
          <div class="term-stack">
            {#each $sessions as session (session.id)}
              <TerminalView
                {session}
                settings={$settings}
                appTheme={$settings.theme}
                active={session.id === $activeSessionId}
                onReconnect={() => void reconnect(session.id)}
                onCloseTab={() => void closeSession(session.id)}
              />
            {/each}
          </div>
        {:else}
          <div class="empty-state">
            <SquareTerminal size={40} strokeWidth={1.4} />
            <p>Select a connection to start a session.</p>
            <p class="hint">Press <span class="kbd">Ctrl K</span> to search everything.</p>
          </div>
        {/if}
      {:else if $activeView === "keys"}
        <KeysView />
      {:else}
        <SettingsView />
      {/if}
    </main>
  </div>
{/if}

{#if paletteOpen}
  <CommandPalette
    onConnect={(connection) => void connect(connection)}
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

{#if hostKeyMismatch}
  <HostKeyModal
    host={hostKeyMismatch.host}
    port={hostKeyMismatch.port}
    storedFingerprint={hostKeyMismatch.storedFingerprint}
    actualFingerprint={hostKeyMismatch.actualFingerprint}
    onUpdate={acceptNewHostKey}
    onCancel={() => (hostKeyMismatch = null)}
  />
{/if}

{#if importOpen}
  <ImportReview
    onClose={() => (importOpen = false)}
    onImported={() => {
      importOpen = false;
      void refreshConnections();
    }}
  />
{/if}

<Toasts />

<style>
  .boot {
    height: 100vh;
    background: var(--bg);
  }

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

  .term-stack {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
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
