<script lang="ts">
  import { onMount } from "svelte";
import { Check } from "@lucide/svelte";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import { api, type ImportCandidate } from "../ipc";
  import { pushToast } from "../stores";
  import Modal from "./Modal.svelte";

  let {
    onClose,
    onImported,
  }: {
    onClose: () => void;
    onImported: () => void;
  } = $props();

  let candidates = $state<ImportCandidate[] | null>(null);
  let selected = $state<Set<number>>(new Set());
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function load(path?: string) {
    busy = true;
    error = null;
    try {
      const found = await api.importSshConfig(path);
      candidates = found;
      selected = new Set(
        found.map((c, i) => (c.duplicate_of === null ? i : -1)).filter((i) => i >= 0),
      );
    } catch (e) {
      error = String((e as Error).message ?? e);
      candidates = null;
    } finally {
      busy = false;
    }
  }

  onMount(() => void load());

  async function pickFile() {
    const picked = await openFileDialog({
      multiple: false,
      directory: false,
      title: "Choose an ssh config file",
    });
    if (typeof picked === "string") await load(picked);
  }

  async function commit() {
    if (!candidates) return;
    const chosen = candidates.filter((_, i) => selected.has(i));
    if (chosen.length === 0) return;
    busy = true;
    try {
      const count = await api.commitImport(chosen);
      pushToast("info", `Imported ${count} connection${count === 1 ? "" : "s"}`);
      onImported();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Import from ssh config" width={620} {onClose}>
  <div class="body">
    <div class="toolbar">
      <span class="hint">
        One-way import: entries become Gapura connections, your ssh config is
        not modified.
      </span>
      <button class="btn" onclick={pickFile}>Choose file...</button>
    </div>

    {#if busy && !candidates}
      <p class="hint">Parsing...</p>
    {:else if error}
      <p class="error-text">{error}</p>
    {:else if candidates === null}
      <p class="hint">Nothing parsed yet.</p>
    {:else if candidates.length === 0}
      <p class="hint">No literal Host entries found in that file.</p>
    {:else}
      <div class="list">
        {#each candidates as candidate, i (i)}
          <label
            class="candidate"
            class:duplicate={candidate.duplicate_of !== null}
            class:selected={selected.has(i)}
          >
            <input
              type="checkbox"
              checked={selected.has(i)}
              onchange={(e) => {
                const next = new Set(selected);
                if (e.currentTarget.checked) next.add(i);
                else next.delete(i);
                selected = next;
              }}
            />
            <span class="c-name">{candidate.name}</span>
            <span class="c-host mono">{candidate.username}@{candidate.host}:{candidate.port}</span>
            <span class="c-notes">
              {#if candidate.duplicate_of !== null}
                <span class="dup">already exists</span>
              {/if}
              {#each candidate.warnings as warning}
                <span class="warn">{warning}</span>
              {/each}
            </span>
          </label>
        {/each}
      </div>

      <footer>
        <span class="hint">{selected.size} of {candidates.length} selected</span>
        <span class="spacer"></span>
        <button class="btn ghost" onclick={onClose}>Cancel</button>
        <button class="btn primary" disabled={busy || selected.size === 0} onclick={commit}>
          <Check size={14} />
          Import {selected.size} selected
        </button>
      </footer>
    {/if}
  </div>
</Modal>

<style>
  .body {
    padding: 2px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 46vh;
    overflow-y: auto;
  }

  .candidate {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 8px 10px;
    border-radius: 6px;
    border: 1px solid var(--border);
    cursor: pointer;
  }

  .candidate.selected {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .candidate.duplicate {
    opacity: 0.6;
  }

  .c-name {
    font-weight: 600;
    min-width: 120px;
  }

  .c-host {
    flex: 1;
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .c-notes {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-width: 160px;
  }

  .dup {
    font-size: 11px;
    color: var(--warning);
  }

  .warn {
    font-size: 11px;
    color: var(--text-faint);
  }

  footer {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .spacer {
    flex: 1;
  }
</style>
