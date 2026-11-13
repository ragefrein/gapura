<script lang="ts">
  import { Lock, KeyRound } from "@lucide/svelte";
  import { api } from "../ipc";
  import { vaultStatus } from "../stores";

  let passphrase = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function unlock() {
    error = null;
    busy = true;
    try {
      await api.vaultUnlock(passphrase || undefined);
      window.dispatchEvent(new CustomEvent("gapura:unlocked"));
    } catch (e) {
      error = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  async function retryKeychain() {
    error = null;
    busy = true;
    try {
      await api.vaultUnlock();
      window.dispatchEvent(new CustomEvent("gapura:unlocked"));
    } catch (e) {
      error = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="unlock">
  <div class="card">
    <span class="mark">
      {#if $vaultStatus.mode === "passphrase"}
        <Lock size={18} />
      {:else}
        <KeyRound size={18} />
      {/if}
    </span>
    <h1>Vault locked</h1>

    {#if $vaultStatus.keychain_error}
      <p class="hint">
        The system keychain refused to hand over the vault key:
        <span class="mono">{$vaultStatus.keychain_error}</span>
        Unlock the keyring (log in, unlock gnome-keyring) and try again.
      </p>
      <button class="btn primary" disabled={busy} onclick={retryKeychain}>
        {busy ? "Unlocking..." : "Retry"}
      </button>
    {:else if $vaultStatus.mode === "passphrase"}
      <form onsubmit={(e) => { e.preventDefault(); unlock(); }}>
        <input
          type="password"
          bind:value={passphrase}
          placeholder="Vault passphrase"
        />
        <button class="btn primary" type="submit" disabled={busy || !passphrase}>
          {busy ? "Unlocking..." : "Unlock"}
        </button>
      </form>
    {:else}
      <p class="hint">Unlocking from the system keychain...</p>
    {/if}

    {#if error}<p class="error-text">{error}</p>{/if}
  </div>
</div>

<style>
  .unlock {
    flex: 1;
    display: grid;
    place-items: center;
    background: var(--bg);
    padding: 24px;
  }

  .card {
    width: 380px;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 26px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    align-items: flex-start;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: var(--accent-soft);
    color: var(--accent);
  }

  h1 {
    font-size: 18px;
  }

  form {
    display: flex;
    gap: 8px;
    width: 100%;
  }

  input {
    flex: 1;
  }

  .hint {
    margin: 0;
    user-select: text;
  }

  .mono {
    font-size: 11.5px;
    word-break: break-all;
  }
</style>
