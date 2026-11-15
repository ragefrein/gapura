<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowRightLeft, Pencil, Plus, Trash2 } from "@lucide/svelte";
  import { listen, api, type Connection, type ForwardKind, type ForwardRule, type ActiveForward } from "../ipc";
  import { pushToast, sessions } from "../stores";
  import Modal from "./Modal.svelte";

  let {
    connection,
    onClose,
  }: {
    connection: Connection;
    onClose: () => void;
  } = $props();

  let rules = $state<ForwardRule[]>([]);
  let active = $state<ActiveForward[]>([]);
  let editing = $state<ForwardRule | null>(null);
  let adding = $state(false);
  let busy = $state(false);

  // form fields
  let kind = $state<ForwardKind>("local");
  let bindHost = $state("127.0.0.1");
  let bindPort = $state(8080);
  let targetHost = $state("localhost");
  let targetPort = $state(3000);
  let autoStart = $state(false);

  let unlisteners: Array<() => void> = [];

  const liveSession = $derived(
    $sessions.find(
      (s) => s.connectionId === connection.id && s.state !== "closed",
    ) ?? null,
  );

  function isActive(ruleId: string): boolean {
    return active.some((a) => a.rule_id === ruleId);
  }

  async function refresh() {
    try {
      rules = await api.listForwardRules(connection.id);
      active = await api.listActiveForwards();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  onMount(() => {
    void refresh();
    void listen<{ rule_id: string; ok: boolean; message: string }>(
      "forward:status",
      (event) => {
        pushToast(event.payload.ok ? "info" : "error", event.payload.message);
        void refresh();
      },
    ).then((off) => unlisteners.push(off));
    return () => {
      for (const off of unlisteners.splice(0)) off();
    };
  });

  function resetForm() {
    adding = false;
    editing = null;
    kind = "local";
    bindHost = "127.0.0.1";
    bindPort = 8080;
    targetHost = "localhost";
    targetPort = 3000;
    autoStart = false;
  }

  function startEdit(rule: ForwardRule) {
    editing = rule;
    adding = false;
    kind = rule.kind;
    bindHost = rule.bind_host;
    bindPort = rule.bind_port;
    targetHost = rule.target_host;
    targetPort = rule.target_port;
    autoStart = rule.auto_start;
  }

  async function saveRule() {
    busy = true;
    try {
      await api.saveForwardRule({
        id: editing?.id ?? null,
        connection_id: connection.id,
        kind,
        bind_host: bindHost,
        bind_port: Number(bindPort) || 0,
        target_host: targetHost,
        target_port: Number(targetPort) || 0,
        enabled: editing ? editing.enabled : true,
        auto_start: autoStart,
      });
      resetForm();
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    } finally {
      busy = false;
    }
  }

  async function removeRule(rule: ForwardRule) {
    try {
      await api.deleteForwardRule(rule.id);
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  async function toggleEnabled(rule: ForwardRule) {
    try {
      await api.saveForwardRule({ ...rule, enabled: !rule.enabled });
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    }
  }

  async function toggleRun(rule: ForwardRule) {
    busy = true;
    try {
      if (isActive(rule.id)) {
        await api.stopForward(rule.id);
        pushToast("info", `Stopped ${rule.bind_host}:${rule.bind_port}`);
      } else if (liveSession) {
        await api.startForward(liveSession.id, rule.id);
        pushToast("info", `Forwarding ${rule.bind_host}:${rule.bind_port}`);
      }
      await refresh();
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Port forwards — {connection.name}" width={560} {onClose}>
  <div class="body">
    <p class="hint">
      Local: Gapura listens here and tunnels through SSH to the target
      (like <span class="mono">-L</span>). Remote: the server listens and
      tunnels back to your machine (<span class="mono">-R</span>). Rules with
      auto-start come up every time the session connects.
    </p>

    {#if rules.length === 0 && !adding && !editing}
      <p class="hint empty">No rules yet.</p>
    {/if}

    {#each rules as rule (rule.id)}
      <div class="rule" class:disabled={!rule.enabled}>
        <div class="rule-main">
          <span class="badge">{rule.kind}</span>
          <span class="pair mono">
            {rule.bind_host}:{rule.bind_port}
            <ArrowRightLeft size={12} />
            {rule.target_host}:{rule.target_port}
          </span>
          {#if rule.auto_start}
            <span class="auto">auto</span>
          {/if}
          {#if isActive(rule.id)}
            <span class="running">running</span>
          {/if}
        </div>
        <div class="rule-tools">
          {#if liveSession}
            <button class="btn small" disabled={busy} onclick={() => toggleRun(rule)}>
              {isActive(rule.id) ? "Stop" : "Start"}
            </button>
          {/if}
          <button
            class="btn small"
            disabled={busy}
            onclick={() => toggleEnabled(rule)}
            title={rule.enabled ? "Disable rule" : "Enable rule"}
          >
            {rule.enabled ? "Enabled" : "Disabled"}
          </button>
          <button class="icon-btn" title="Edit" onclick={() => startEdit(rule)}>
            <Pencil size={14} />
          </button>
          <button class="icon-btn danger" title="Delete" onclick={() => removeRule(rule)}>
            <Trash2 size={14} />
          </button>
        </div>
      </div>
    {/each}

    {#if adding || editing}
      <form
        class="editor"
        onsubmit={(e) => {
          e.preventDefault();
          saveRule();
        }}
      >
        <div class="row">
          <div class="field">
            <label for="f-kind">Kind</label>
            <select id="f-kind" bind:value={kind}>
              <option value="local">Local (-L)</option>
              <option value="remote">Remote (-R)</option>
            </select>
          </div>
          <div class="field">
            <label for="f-auto">Start on connect</label>
            <input id="f-auto" type="checkbox" bind:checked={autoStart} />
          </div>
        </div>
        <div class="row">
          <div class="field grow">
            <label for="f-bind">{kind === "local" ? "Listen here (host:port)" : "Server listens (host:port)"}</label>
            <div class="pair-input">
              <input class="mono" bind:value={bindHost} spellcheck="false" />
              <input class="mono port" type="number" min="1" max="65535" bind:value={bindPort} />
            </div>
          </div>
          <div class="field grow">
            <label for="f-target">{kind === "local" ? "Target via server (host:port)" : "Target on this machine"}</label>
            <div class="pair-input">
              <input class="mono" bind:value={targetHost} spellcheck="false" />
              <input class="mono port" type="number" min="1" max="65535" bind:value={targetPort} />
            </div>
          </div>
        </div>
        <div class="editor-actions">
          <button type="button" class="btn ghost" onclick={resetForm}>Cancel</button>
          <button type="submit" class="btn primary" disabled={busy}>
            {editing ? "Save rule" : "Add rule"}
          </button>
        </div>
      </form>
    {:else}
      <button class="btn add" onclick={() => (adding = true)}>
        <Plus size={15} />
        Add rule
      </button>
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

  .empty {
    margin: 4px 0;
  }

  .rule {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .rule.disabled {
    opacity: 0.55;
  }

  .rule-main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex-wrap: wrap;
  }

  .badge {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 2px 7px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 700;
  }

  .pair {
    font-size: 12px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
    user-select: text;
  }

  .auto,
  .running {
    font-size: 11px;
    color: var(--text-muted);
  }

  .running {
    color: var(--accent);
    font-weight: 600;
  }

  .rule-tools {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .btn.small {
    padding: 3px 9px;
    font-size: 11.5px;
  }

  .editor {
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .row {
    display: flex;
    gap: 10px;
    align-items: flex-end;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field.grow {
    flex: 1;
  }

  .field label {
    font-size: 11.5px;
    color: var(--text-muted);
  }

  .pair-input {
    display: flex;
    gap: 6px;
  }

  .pair-input input:first-child {
    flex: 1;
    min-width: 0;
  }

  .port {
    width: 84px;
  }

  .editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .add {
    align-self: flex-start;
  }
</style>
