<script lang="ts">
  import { X } from "@lucide/svelte";

  let {
    title,
    width = 460,
    onClose,
    children,
  }: {
    title: string;
    width?: number;
    onClose: () => void;
    children?: import("svelte").Snippet;
  } = $props();

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") onClose();
  }
</script>

<svelte:window on:keydown={onKeydown} />

<div
  class="overlay"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div class="modal" style="width: {width}px" role="dialog" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="icon-btn" onclick={onClose} title="Close">
        <X size={16} />
      </button>
    </header>
    {@render children?.()}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(6, 9, 13, 0.55);
    display: grid;
    place-items: center;
    z-index: 60;
  }

  :root[data-theme="light"] .overlay {
    background: rgba(30, 36, 44, 0.35);
  }

  .modal {
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px 10px;
  }

  header h2 {
    font-size: 15px;
  }
</style>
