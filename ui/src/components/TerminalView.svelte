<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { Search, RefreshCw, WifiOff, X, ArrowUp, ArrowDown } from "@lucide/svelte";
  import "@xterm/xterm/css/xterm.css";

  import { api, type Settings, type Theme } from "../ipc";
  import { registerWriter, unregisterWriter } from "../terminalBus";
  import { DARK_THEME, LIGHT_THEME } from "../terminalTheme";
  import type { SessionTab } from "../stores";

  let {
    session,
    settings,
    appTheme,
    active,
    onReconnect,
    onCloseTab,
  }: {
    session: SessionTab;
    settings: Settings;
    appTheme: Theme;
    active: boolean;
    onReconnect: () => void;
    onCloseTab: () => void;
  } = $props();

  let container: HTMLDivElement | undefined = $state();
  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let searchAddon: SearchAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let findOpen = $state(false);
  let findText = $state("");
  let findInput: HTMLInputElement | undefined = $state();

  // Debounce guard so rapid resize events do not spam the IPC surface.
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;

  function currentTheme() {
    return appTheme === "light" ? LIGHT_THEME : DARK_THEME;
  }

  function bytesToBase64(bytes: Uint8Array): string {
    let binary = "";
    const CHUNK = 0x8000;
    for (let i = 0; i < bytes.length; i += CHUNK) {
      binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
    }
    return btoa(binary);
  }

  function scheduleFit(sendResize: boolean) {
    if (!fitAddon || !term || !container) return;
    if (resizeTimer) clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      if (!fitAddon || !term) return;
      try {
        fitAddon.fit();
        if (sendResize) {
          void api.sessionResize(session.id, term.cols, term.rows);
        }
      } catch {
        // Container may be hidden (display:none) during tab switches.
      }
    }, 60);
  }

  onMount(() => {
    const t = new Terminal({
      fontFamily: `${settings.terminal_font_family}, "Cascadia Code", "SF Mono", Consolas, monospace`,
      fontSize: settings.terminal_font_size,
      scrollback: settings.scrollback_lines,
      cursorBlink: true,
      macOptionIsMeta: true,
      allowProposedApi: true,
      theme: currentTheme(),
    });
    term = t;

    fitAddon = new FitAddon();
    t.loadAddon(fitAddon);
    searchAddon = new SearchAddon();
    t.loadAddon(searchAddon);
    t.loadAddon(new Unicode11Addon());
    t.unicode.activeVersion = "unicode11";

    try {
      t.loadAddon(new WebglAddon());
    } catch {
      // WebGL unavailable (headless, blocked GPU): the DOM renderer is fine.
    }

    registerWriter(session.id, (data) => t.write(data));

    t.onData((data) => {
      void api.sessionWrite(session.id, bytesToBase64(new TextEncoder().encode(data)));
    });

    t.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const ctrlShift = event.ctrlKey && event.shiftKey;
      if (ctrlShift && event.key.toLowerCase() === "c") {
        const selection = t.getSelection();
        if (selection) {
          void navigator.clipboard.writeText(selection);
          return false;
        }
      }
      if (ctrlShift && event.key.toLowerCase() === "v") {
        void navigator.clipboard.readText().then((text) => {
          if (text) t.paste(text);
        });
        return false;
      }
      if (ctrlShift && event.key.toLowerCase() === "f") {
        findOpen = true;
        setTimeout(() => findInput?.focus(), 30);
        return false;
      }
      return true;
    });

    t.open(container!);
    scheduleFit(false);

    resizeObserver = new ResizeObserver(() => scheduleFit(true));
    resizeObserver.observe(container!);

    return () => {
      unregisterWriter(session.id);
    };
  });

  // React to settings/theme changes without losing scrollback.
  $effect(() => {
    if (term) {
      term.options.theme = currentTheme();
      term.options.fontFamily = `${settings.terminal_font_family}, "Cascadia Code", "SF Mono", Consolas, monospace`;
      term.options.fontSize = settings.terminal_font_size;
      scheduleFit(true);
    }
  });

  $effect(() => {
    if (active) scheduleFit(true);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    if (resizeTimer) clearTimeout(resizeTimer);
    term?.dispose();
    term = null;
  });

  function findNext(backwards = false) {
    if (!searchAddon || !findText) return;
    if (backwards) searchAddon.findPrevious(findText);
    else searchAddon.findNext(findText);
  }

  function closeFind() {
    searchAddon?.clearDecorations();
    findOpen = false;
  }
</script>

<div class="terminal-pane" class:hidden={!active}>
  {#if session.state === "closed"}
    <div class="banner">
      <WifiOff size={16} />
      <span class="banner-text">
        Connection closed{session.closedReason ? ` — ${session.closedReason}` : ""}.
        Scrollback is kept.
      </span>
      <button class="btn primary small" onclick={onReconnect}>
        <RefreshCw size={14} />
        Reconnect
      </button>
      <button class="btn small" onclick={onCloseTab}>
        <X size={14} />
        Close tab
      </button>
    </div>
  {:else if session.state === "connecting"}
    <div class="banner connecting">
      Connecting to {session.name}...
    </div>
  {/if}

  {#if findOpen}
    <div class="find">
      <Search size={13} />
      <input
        bind:this={findInput}
        bind:value={findText}
        placeholder="Find in output"
        onkeydown={(e) => {
          if (e.key === "Enter") findNext(e.shiftKey);
          if (e.key === "Escape") closeFind();
        }}
        spellcheck="false"
      />
      <button class="icon-btn" title="Previous (Shift+Enter)" onclick={() => findNext(true)}>
        <ArrowUp size={13} />
      </button>
      <button class="icon-btn" title="Next (Enter)" onclick={() => findNext(false)}>
        <ArrowDown size={13} />
      </button>
      <button class="icon-btn" title="Close" onclick={closeFind}>
        <X size={13} />
      </button>
    </div>
  {/if}

  <div class="term-host" bind:this={container}></div>
</div>

<style>
  .terminal-pane {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
    padding: 0 0 0 6px;
  }

  .terminal-pane.hidden {
    display: none;
  }

  .term-host {
    flex: 1;
    min-width: 0;
    padding: 6px 0 6px 0;
  }

  .term-host :global(.xterm) {
    height: 100%;
  }

  .banner {
    position: absolute;
    top: 0;
    left: 6px;
    right: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    background: var(--danger-soft);
    border-bottom: 1px solid var(--border);
    color: var(--danger);
    font-size: 12.5px;
  }

  .banner.connecting {
    background: var(--bg-elev);
    color: var(--text-muted);
    border-bottom: 1px solid var(--border);
  }

  .banner-text {
    flex: 1;
  }

  .btn.small {
    padding: 4px 10px;
    font-size: 12px;
  }

  .find {
    position: absolute;
    top: 8px;
    right: 12px;
    z-index: 6;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 5px 8px;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    color: var(--text-faint);
  }

  .find input {
    width: 180px;
    padding: 3px 6px;
    font-size: 12px;
  }
</style>
