<script lang="ts">
  import { ShieldAlert } from "@lucide/svelte";
  import Modal from "./Modal.svelte";

  let {
    host,
    port,
    storedFingerprint,
    actualFingerprint,
    onUpdate,
    onCancel,
  }: {
    host: string;
    port: number;
    storedFingerprint: string;
    actualFingerprint: string;
    onUpdate: () => void;
    onCancel: () => void;
  } = $props();
</script>

<Modal title="Server fingerprint changed" width={480} onClose={onCancel}>
  <div class="body">
    <div class="warning">
      <ShieldAlert size={22} />
      <p>
        {host}:{port} presented a different host key than the one pinned on a
        previous visit. This can mean the server was reinstalled — but it can
        also mean someone is intercepting the connection. If you cannot
        explain the change, stop here.
      </p>
    </div>

    <dl>
      <div>
        <dt>Pinned fingerprint</dt>
        <dd class="mono old">{storedFingerprint}</dd>
      </div>
      <div>
        <dt>New fingerprint</dt>
        <dd class="mono new">{actualFingerprint}</dd>
      </div>
    </dl>

    <footer>
      <button class="btn ghost" onclick={onCancel}>Cancel</button>
      <button class="btn danger" onclick={onUpdate}>
        Update fingerprint and connect
      </button>
    </footer>
  </div>
</Modal>

<style>
  .body {
    padding: 2px 16px 16px;
  }

  .warning {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--danger-soft);
    margin-bottom: 14px;
  }

  .warning :global(svg) {
    color: var(--danger);
    flex: none;
    margin-top: 2px;
  }

  .warning p {
    margin: 0;
    font-size: 12.5px;
    color: var(--text);
    user-select: text;
  }

  dl {
    margin: 0 0 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  dt {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-faint);
    margin-bottom: 2px;
  }

  dd {
    margin: 0;
    font-size: 12px;
    word-break: break-all;
    user-select: text;
  }

  dd.old {
    color: var(--text-muted);
  }

  dd.new {
    color: var(--warning);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
