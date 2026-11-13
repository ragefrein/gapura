<script lang="ts">
  import { Eye, EyeOff } from "@lucide/svelte";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import { api, type Connection } from "../ipc";
  import { groups, pushToast } from "../stores";
  import Modal from "./Modal.svelte";

  let {
    connection,
    onClose,
    onSaved,
  }: {
    connection: Connection | null;
    onClose: () => void;
    onSaved: (connection: Connection) => void;
  } = $props();

  const COLOR_SWATCHES = [
    "#14b8a6",
    "#3b82f6",
    "#8b5cf6",
    "#ec4899",
    "#f5a524",
    "#e5484d",
    "#84cc16",
    "#64748b",
  ];

  // The form intentionally captures the connection as it was when opened.
  // svelte-ignore state_referenced_locally
  let name = $state(connection?.name ?? "");
  // svelte-ignore state_referenced_locally
  let host = $state(connection?.host ?? "");
  // svelte-ignore state_referenced_locally
  let port = $state(connection?.port ?? 22);
  // svelte-ignore state_referenced_locally
  let username = $state(connection?.username ?? "");
  // svelte-ignore state_referenced_locally
  let authKind = $state<"agent" | "password" | "key_file">(
    connection?.auth.kind ?? "agent",
  );
  let password = $state("");
  // svelte-ignore state_referenced_locally
  let keyPath = $state(
    connection?.auth.kind === "key_file" ? connection.auth.data.path : "",
  );
  let keyPassphrase = $state("");
  // svelte-ignore state_referenced_locally
  let groupId = $state<string | null>(connection?.group_id ?? null);
  // svelte-ignore state_referenced_locally
  let color = $state(connection?.color ?? null);
  // svelte-ignore state_referenced_locally
  let tagsText = $state(connection?.tags.join(", ") ?? "");
  // svelte-ignore state_referenced_locally
  let notes = $state(connection?.notes ?? "");

  let saving = $state(false);
  let error = $state<string | null>(null);
  let showPassword = $state(false);
  let newGroupName = $state("");
  let creatingGroup = $state(false);

  const isEdit = $derived(connection !== null);

  async function browseKey() {
    const picked = await openFileDialog({
      multiple: false,
      directory: false,
      title: "Choose a private key",
    });
    if (typeof picked === "string") keyPath = picked;
  }

  async function createGroup() {
    const name = newGroupName.trim();
    if (!name) return;
    try {
      const group = await api.saveGroup(null, name);
      groups.update((list) => [...list, group]);
      groupId = group.id;
      newGroupName = "";
      creatingGroup = false;
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  async function save() {
    error = null;
    saving = true;
    try {
      const auth = (() => {
        if (authKind === "agent") return { kind: "agent" } as const;
        if (authKind === "password")
          return {
            kind: "password",
            data: { password: password || null },
          } as const;
        return {
          kind: "key_file",
          data: { path: keyPath, passphrase: keyPassphrase || null },
        } as const;
      })();

      const saved = await api.saveConnection({
        id: connection?.id ?? null,
        name,
        group_id: groupId,
        host,
        port: Number(port) || 22,
        username,
        auth,
        tags: tagsText
          .split(",")
          .map((t) => t.trim())
          .filter(Boolean),
        color,
        notes,
      });
      onSaved(saved);
    } catch (e) {
      error = String((e as Error).message ?? e);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={isEdit ? "Edit connection" : "New connection"} width={480} {onClose}>
  <form class="form" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <div class="row two">
      <div class="field">
        <label for="f-name">Name</label>
        <input id="f-name" bind:value={name} placeholder="prod-vps" required spellcheck="false" />
      </div>
      <div class="field">
        <label for="f-group">Folder</label>
        <select id="f-group" bind:value={groupId}>
          <option value={null}>None</option>
          {#each $groups as group (group.id)}
            <option value={group.id}>{group.name}</option>
          {/each}
          <option value="__new">{creatingGroup ? "Name:" : "New folder..."}</option>
        </select>
      </div>
    </div>

    {#if groupId === "__new"}
      <div class="new-group">
        <input bind:value={newGroupName} placeholder="Folder name" spellcheck="false" />
        <button type="button" class="btn" onclick={createGroup}>Create</button>
      </div>
    {/if}

    <div class="row host-row">
      <div class="field grow">
        <label for="f-host">Host</label>
        <input id="f-host" bind:value={host} placeholder="203.0.113.10 or db.example.com" required spellcheck="false" />
      </div>
      <div class="field port">
        <label for="f-port">Port</label>
        <input id="f-port" type="number" min="1" max="65535" bind:value={port} />
      </div>
    </div>

    <div class="row two">
      <div class="field">
        <label for="f-user">Username</label>
        <input id="f-user" bind:value={username} placeholder="root" required spellcheck="false" />
      </div>
      <div class="field">
        <label for="f-auth">Authentication</label>
        <select id="f-auth" bind:value={authKind}>
          <option value="agent">SSH agent</option>
          <option value="password">Password</option>
          <option value="key_file">Key file</option>
        </select>
      </div>
    </div>

    {#if authKind === "password"}
      <div class="field">
        <label for="f-password">
          {isEdit && !password ? "Password (leave blank to keep)" : "Password"}
        </label>
        <div class="password">
          <input
            id="f-password"
            type={showPassword ? "text" : "password"}
            bind:value={password}
            placeholder="••••••••"
            spellcheck="false"
          />
          <button
            type="button"
            class="icon-btn"
            title={showPassword ? "Hide" : "Show"}
            onclick={() => (showPassword = !showPassword)}
          >
            {#if showPassword}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
          </button>
        </div>
      </div>
    {:else if authKind === "key_file"}
      <div class="field">
        <label for="f-key">Private key file</label>
        <div class="key-row">
          <input id="f-key" bind:value={keyPath} class="mono" placeholder="~/.ssh/id_ed25519" spellcheck="false" />
          <button type="button" class="btn" onclick={browseKey}>Browse</button>
        </div>
      </div>
      <div class="field">
        <label for="f-pass">
          {isEdit && !keyPassphrase ? "Key passphrase (leave blank to keep)" : "Key passphrase (optional)"}
        </label>
        <input id="f-pass" type="password" bind:value={keyPassphrase} placeholder="••••••••" spellcheck="false" />
      </div>
    {/if}

    <div class="field">
      <span class="swatch-label">Color</span>
      <div class="swatches">
        <button
          type="button"
          class="swatch none"
          class:selected={color === null}
          title="No color"
          onclick={() => (color = null)}
        ></button>
        {#each COLOR_SWATCHES as swatch (swatch)}
          <button
            type="button"
            class="swatch"
            class:selected={color === swatch}
            style="background: {swatch}"
            title={swatch}
            onclick={() => (color = swatch)}
          ></button>
        {/each}
      </div>
    </div>

    <div class="field">
      <label for="f-tags">Tags (comma separated)</label>
      <input id="f-tags" bind:value={tagsText} placeholder="prod, database" spellcheck="false" />
    </div>

    <div class="field">
      <label for="f-notes">Notes</label>
      <textarea id="f-notes" bind:value={notes} rows="3" placeholder="What lives on this box"></textarea>
    </div>

    {#if error}
      <p class="error-text">{error}</p>
    {/if}

    <footer>
      <button type="button" class="btn ghost" onclick={onClose}>Cancel</button>
      <button type="submit" class="btn primary" disabled={saving}>
        {saving ? "Saving..." : isEdit ? "Save changes" : "Create connection"}
      </button>
    </footer>
  </form>
</Modal>

<style>
  .form {
    padding: 2px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
  }

  .row {
    display: flex;
    gap: 10px;
  }

  .row.two > .field {
    flex: 1;
  }

  .host-row {
    align-items: flex-end;
  }

  .grow {
    flex: 1;
  }

  .port {
    width: 88px;
    flex: none;
  }

  .new-group {
    display: flex;
    gap: 8px;
    margin-top: -6px;
  }

  .password {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .password input {
    flex: 1;
  }

  .key-row {
    display: flex;
    gap: 8px;
  }

  .key-row input {
    flex: 1;
  }

  .swatch-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .swatches {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid transparent;
  }

  .swatch.none {
    background: var(--bg-input);
    border-color: var(--border-strong);
  }

  .swatch.selected {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }

  textarea {
    resize: vertical;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
