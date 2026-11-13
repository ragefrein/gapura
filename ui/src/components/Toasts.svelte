<script lang="ts">
  import { fly } from "svelte/transition";
  import { CircleAlert, Check } from "@lucide/svelte";
  import { dismissToast, toasts } from "../stores";
</script>

<div class="toasts">
  {#each $toasts as toast (toast.id)}
    <div
      class="toast"
      class:error={toast.kind === "error"}
      transition:fly={{ y: 8, duration: 150 }}
      role="status"
    >
      {#if toast.kind === "error"}
        <CircleAlert size={15} />
      {:else}
        <Check size={15} />
      {/if}
      <span>{toast.message}</span>
      <button class="icon-btn" onclick={() => dismissToast(toast.id)} title="Dismiss">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
      </button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 90;
    max-width: 380px;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    box-shadow: var(--shadow);
    color: var(--text);
  }

  .toast :global(svg) {
    color: var(--accent);
    flex: none;
  }

  .toast.error :global(svg) {
    color: var(--danger);
  }

  .toast span {
    flex: 1;
    font-size: 12.5px;
  }

  .toast .icon-btn {
    width: 20px;
    height: 20px;
  }
</style>
