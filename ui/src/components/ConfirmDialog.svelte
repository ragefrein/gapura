<script lang="ts">
  import Modal from "./Modal.svelte";

  let {
    title,
    message,
    confirmLabel = "Delete",
    onConfirm,
    onCancel,
  }: {
    title: string;
    message: string;
    confirmLabel?: string;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();
</script>

<Modal {title} width={400} onClose={onCancel}>
  <div class="body">
    <p>{message}</p>
    <footer>
      <button class="btn ghost" onclick={onCancel}>Cancel</button>
      <button class="btn danger" onclick={onConfirm}>{confirmLabel}</button>
    </footer>
  </div>
</Modal>

<style>
  .body {
    padding: 2px 16px 16px;
  }

  p {
    margin: 0 0 14px;
    color: var(--text-muted);
    user-select: text;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
