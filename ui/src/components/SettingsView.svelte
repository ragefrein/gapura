<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Trash2, HardDrive, ExternalLink, RefreshCw, Sun, Moon, Info } from "@lucide/svelte";
  import { api, type KnownHostEntry, type Settings, type Theme } from "../ipc";
  import { pushToast, settings } from "../stores";
  import ConfirmDialog from "./ConfirmDialog.svelte";

  const FONT_SUGGESTIONS = ["JetBrains Mono", "Cascadia Code", "Fira Code", "SF Mono", "Consolas", "DejaVu Sans Mono"];

  let knownHosts = $state<KnownHostEntry[]>([]);
  let version = $state("");
  let paths = $state<{ data_dir: string; config_dir: string } | null>(null);
  let confirmForget = $state<KnownHostEntry | null>(null);
  let localSettings: Settings = $state({ ...$settings });

  async function refresh() {
    try {
      knownHosts = await api.listKnownHosts();
      paths = await api.appPaths();
      version = (await api.versionInfo()).version;
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  onMount(refresh);

  async function save() {
    try {
      await api.saveSettings(localSettings);
      settings.set({ ...localSettings });
      pushToast("info", "Settings saved");
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  function setTheme(theme: Theme) {
    localSettings = { ...localSettings, theme };
    void save();
    window.dispatchEvent(new CustomEvent("gapura:theme-set", { detail: theme }));
  }

  async function forgetHost() {
    const target = confirmForget;
    if (!target) return;
    confirmForget = null;
    try {
      await api.removeKnownHost(target.host_port);
      await refresh();
      pushToast("info", `Forgot ${target.host_port}; it will be pinned again on the next connect`);
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  let cleanup: (() => void) | undefined;
  onMount(() => {
    const handler = (event: Event) => {
      localSettings = { ...localSettings, theme: (event as CustomEvent<Theme>).detail };
    };
    window.addEventListener("gapura:theme-set", handler);
    cleanup = () => window.removeEventListener("gapura:theme-set", handler);
  });
  onDestroy(() => cleanup?.());
</script>

<div class="page">
  <div class="group">
    <div class="rows">
      <div class="row">
        <span class="label">Theme</span>
        <div class="theme-toggle">
          <button
            class="btn"
            class:active={localSettings.theme === "dark"}
            onclick={() => setTheme("dark")}
          >
            <Moon size={14} />
            Dark
          </button>
          <button
            class="btn"
            class:active={localSettings.theme === "light"}
            onclick={() => setTheme("light")}
          >
            <Sun size={14} />
            Light
          </button>
        </div>
      </div>
    </div>
  </div>

  <div class="group">
    <div class="rows">
      <div class="row">
        <span class="label">Terminal font</span>
        <div class="control">
          <input class="mono" list="font-suggestions" bind:value={localSettings.terminal_font_family} />
          <datalist id="font-suggestions">
            {#each FONT_SUGGESTIONS as font (font)}
              <option value={font}></option>
            {/each}
          </datalist>
        </div>
        <div class="control small">
          <input
            type="number"
            min="8"
            max="32"
            bind:value={localSettings.terminal_font_size}
            title="Font size"
          />
        </div>
      </div>
      <div class="row">
        <span class="label">Scrollback lines</span>
        <div class="control small">
          <input type="number" min="100" max="200000" step="500" bind:value={localSettings.scrollback_lines} />
        </div>
      </div>
    </div>
    <div class="actions">
      <button class="btn primary" onclick={save}>Save</button>
    </div>
  </div>

  <div class="group">
    <div class="group-head">
      <h3>Pinned server fingerprints</h3>
      <button class="icon-btn" title="Refresh" onclick={refresh}>
        <RefreshCw size={14} />
      </button>
    </div>
    {#if knownHosts.length === 0}
      <p class="hint">No hosts pinned yet. The first connection to a server records its
      fingerprint here; a change later blocks the connection until you decide.</p>
    {:else}
      <div class="rows">
        {#each knownHosts as entry (entry.host_port)}
          <div class="row">
            <div class="info">
              <span class="host mono">{entry.host_port}</span>
              <span class="fp mono">{entry.fingerprint}</span>
            </div>
            <button
              class="icon-btn danger"
              title="Forget this host"
              onclick={() => (confirmForget = entry)}
            >
              <Trash2 size={14} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <div class="group">
    <div class="group-head">
      <h3>About</h3>
    </div>
    <div class="about">
      <span class="about-line">
        <Info size={14} />
        Gapura v{version} — no automatic updates in v0.1; check releases manually.
      </span>
      <div class="about-tools">
        <button
          class="btn"
          title="Open the data folder"
          onclick={() => void api.openDataDir()}
        >
          <HardDrive size={14} />
          Data folder
        </button>
        <a
          class="btn"
          href="https://github.com/ragefrein/gapura/releases"
          target="_blank"
          rel="noreferrer"
        >
          <ExternalLink size={14} />
          Releases
        </a>
      </div>
      {#if paths}
        <p class="hint mono path-line">Vault: {paths.data_dir}</p>
      {/if}
    </div>
  </div>
</div>

{#if confirmForget}
  <ConfirmDialog
    title="Forget pinned fingerprint"
    message={`Forget the fingerprint for ${confirmForget.host_port}? The next connect will pin whatever key the server presents then.`}
    confirmLabel="Forget"
    onConfirm={forgetHost}
    onCancel={() => (confirmForget = null)}
  />
{/if}

<style>
  .page {
    flex: 1;
    overflow-y: auto;
    padding: 22px 26px;
    display: flex;
    flex-direction: column;
    gap: 22px;
    max-width: 860px;
  }

  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  h3 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.7px;
    color: var(--text-faint);
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 14px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .label {
    width: 160px;
    color: var(--text-muted);
  }

  .theme-toggle {
    display: flex;
    gap: 6px;
  }

  .theme-toggle .btn.active {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-soft);
  }

  .control {
    flex: 1;
    max-width: 320px;
  }

  .control input {
    width: 100%;
  }

  .control.small {
    flex: none;
    width: 90px;
  }

  .actions {
    margin-top: 10px;
    display: flex;
    justify-content: flex-end;
    max-width: 860px;
  }

  .info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .host {
    font-weight: 600;
    font-size: 12.5px;
  }

  .fp {
    font-size: 11.5px;
    color: var(--text-muted);
    word-break: break-all;
    user-select: text;
  }

  .about {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .about-line {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
  }

  .about-tools {
    display: flex;
    gap: 8px;
  }

  .about-tools a {
    text-decoration: none;
    color: inherit;
  }

  .path-line {
    word-break: break-all;
    user-select: text;
  }
</style>
