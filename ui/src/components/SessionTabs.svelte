<script lang="ts">
  import { X } from "@lucide/svelte";
  import { activeSessionId, sessions } from "../stores";

  function select(id: string) {
    activeSessionId.set(id);
  }

  function close(id: string) {
    window.dispatchEvent(new CustomEvent("gapura:close-session", { detail: id }));
  }
</script>

{#if $sessions.length > 0}
  <div class="tabbar">
    {#each $sessions as session (session.id)}
      <div
        class="tab"
        class:active={session.id === $activeSessionId}
        role="tab"
        tabindex="0"
        aria-selected={session.id === $activeSessionId}
        onclick={() => select(session.id)}
        onkeydown={(e) => {
          if (e.key === "Enter") select(session.id);
        }}
      >
        <span
          class="dot"
          class:closed={session.state === "closed"}
          class:connecting={session.state === "connecting"}
          style={session.color ? `background: ${session.color}` : ""}
        ></span>
        <span class="name" title={session.name}>{session.name}</span>
        <button
          class="icon-btn"
          title="Close session"
          onclick={(e) => {
            e.stopPropagation();
            close(session.id);
          }}
        >
          <X size={12} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .tabbar {
    display: flex;
    align-items: stretch;
    gap: 4px;
    padding: 6px 8px 0;
    background: var(--bg-elev);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    flex: none;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 8px 7px 11px;
    border-radius: 8px 8px 0 0;
    color: var(--text-muted);
    cursor: pointer;
    max-width: 220px;
    border: 1px solid transparent;
    border-bottom: none;
    font-size: 12.5px;
  }

  .tab.active {
    color: var(--text);
    background: var(--bg);
    border-color: var(--border);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-faint);
    flex: none;
  }

  .dot.connecting {
    background: var(--warning) !important;
    animation: pulse 1s infinite alternate;
  }

  .dot.closed {
    background: var(--danger) !important;
  }

  @keyframes pulse {
    from { opacity: 0.4; }
    to { opacity: 1; }
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon-btn {
    width: 18px;
    height: 18px;
    border-radius: 4px;
  }
</style>
