<script lang="ts">
  import { onMount } from "svelte";
  import { Copy, Plus, Trash2 } from "@lucide/svelte";
  import { api, type SshKey } from "../ipc";
  import { pushToast } from "../stores";
  import Modal from "./Modal.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";

  let keys = $state<SshKey[]>([]);
  let generateOpen = $state(false);
  let newName = $state("id_ed25519_gapura");
  let newPassphrase = $state("");
  let busy = $state(false);
  let confirmDelete = $state<SshKey | null>(null);

  async function refresh() {
    try {
      keys = await api.listKeys();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  onMount(refresh);

  async function generate() {
    busy = true;
    try {
      const key = await api.generateKey(newName, newPassphrase || undefined);
      pushToast("info", `Generated ${key.name}`);
      generateOpen = false;
      newPassphrase = "";
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    } finally {
      busy = false;
    }
  }

  async function copyPublicKey(key: SshKey) {
    try {
      await navigator.clipboard.writeText(key.public_key);
      pushToast("info", "Public key copied");
    } catch {
      pushToast("error", "Clipboard is not available");
    }
  }

  async function removeKey() {
    const target = confirmDelete;
    if (!target) return;
    confirmDelete = null;
    try {
      await api.deleteKey(target.id);
      await refresh();
      pushToast("info", `Removed ${target.name} from the list (file kept on disk)`);
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }
</script>

<div class="page">
  <div class="head">
    <h2>Keys</h2>
    <button class="btn primary" onclick={() => (generateOpen = true)}>
      <Plus size={15} />
      Generate key
    </button>
  </div>

  {#if keys.length === 0}
    <p class="hint">No keys registered yet. Generated and imported keys show up here with
    their fingerprints; the vault never stores the private key itself.</p>
  {:else}
    <div class="rows">
      {#each keys as key (key.id)}
        <div class="row">
          <div class="info">
            <span class="name">{key.name}</span>
            <span class="fp mono">{key.fingerprint}</span>
            <span class="path hint mono">{key.path}{key.has_passphrase ? " (passphrase-protected)" : ""}</span>
          </div>
          <div class="tools">
            <button class="icon-btn" title="Copy public key" onclick={() => copyPublicKey(key)}>
              <Copy size={15} />
            </button>
            <button
              class="icon-btn danger"
              title="Remove from list"
              onclick={() => (confirmDelete = key)}
            >
              <Trash2 size={15} />
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if generateOpen}
  <Modal title="Generate ed25519 key" width={420} onClose={() => (generateOpen = false)}>
    <div class="form">
      <div class="field">
        <label for="k-name">File name (inside ~/.ssh)</label>
        <input id="k-name" class="mono" bind:value={newName} spellcheck="false" />
      </div>
      <div class="field">
        <label for="k-pass">Passphrase (optional)</label>
        <input id="k-pass" type="password" bind:value={newPassphrase} placeholder="••••••••" />
      </div>
      <footer>
        <button class="btn ghost" onclick={() => (generateOpen = false)}>Cancel</button>
        <button class="btn primary" disabled={busy} onclick={generate}>
          {busy ? "Generating..." : "Generate"}
        </button>
      </footer>
    </div>
  </Modal>
{/if}

{#if confirmDelete}
  <ConfirmDialog
    title="Remove key"
    message={`Remove "${confirmDelete.name}" from Gapura's list? The key file stays on disk untouched.`}
    confirmLabel="Remove"
    onConfirm={removeKey}
    onCancel={() => (confirmDelete = null)}
  />
{/if}

<style>
  .page {
    flex: 1;
    overflow-y: auto;
    padding: 22px 26px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  h2 {
    font-size: 16px;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 760px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    font-weight: 600;
  }

  .fp {
    font-size: 12px;
    color: var(--accent);
    user-select: text;
  }

  .path {
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .tools {
    display: flex;
    gap: 4px;
  }

  .form {
    padding: 2px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .hint {
    max-width: 640px;
  }
</style>
