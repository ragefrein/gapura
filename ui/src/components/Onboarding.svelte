<script lang="ts">
  import { fade } from "svelte/transition";
  import {
    KeyRound,
    Lock,
    Server,
    Check,
    FileUp,
    Terminal,
    ArrowRight,
  } from "@lucide/svelte";
  import { api, type ImportCandidate } from "../ipc";
  import { pushToast } from "../stores";

  type Step = "mode" | "import" | "keys" | "done";

  let step = $state<Step>("mode");
  let chosenMode = $state<"keychain" | "passphrase">("keychain");
  let passphrase = $state("");
  let passphraseConfirm = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  let candidates = $state<ImportCandidate[] | null>(null);
  let selected = $state<Set<number>>(new Set());

  let keyName = $state("id_ed25519_gapura");
  let keyPassphrase = $state("");

  async function setupVault() {
    error = null;
    if (chosenMode === "passphrase") {
      if (passphrase.length < 8) {
        error = "Use at least 8 characters.";
        return;
      }
      if (passphrase !== passphraseConfirm) {
        error = "The two passphrases do not match.";
        return;
      }
    }
    busy = true;
    try {
      await api.vaultSetup(chosenMode, chosenMode === "passphrase" ? passphrase : undefined);
      step = "import";
      try {
        const found = await api.importSshConfig();
        candidates = found.length > 0 ? found : null;
        selected = new Set(
          found.map((c, i) => (c.duplicate_of === null ? i : -1)).filter((i) => i >= 0),
        );
      } catch {
        candidates = null;
      }
    } catch (e) {
      error = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  async function commitImport() {
    if (!candidates) return;
    const chosen = candidates.filter((_, i) => selected.has(i));
    if (chosen.length === 0) return;
    busy = true;
    try {
      const count = await api.commitImport(chosen);
      pushToast("info", `Imported ${count} connection${count === 1 ? "" : "s"}`);
      step = "keys";
    } catch (e) {
      pushToast("error", String((e as Error).message ?? e));
    } finally {
      busy = false;
    }
  }

  async function generateKey() {
    busy = true;
    error = null;
    try {
      const key = await api.generateKey(keyName, keyPassphrase || undefined);
      pushToast("info", `Generated ${key.name} (${key.fingerprint.slice(0, 17)}...)`);
      step = "done";
    } catch (e) {
      error = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function finish() {
    window.dispatchEvent(new CustomEvent("gapura:onboarded"));
  }
</script>

<div class="wizard">
  <div class="card" in:fade={{ duration: 150 }}>
    <header>
      <span class="mark"><Terminal size={18} /></span>
      <h1>Welcome to Gapura</h1>
      <p class="hint">
        Your connections and secrets are stored encrypted on this machine.
        {step === "mode" ? "Pick how the vault unlocks." : ""}
      </p>
    </header>

    {#if step === "mode"}
      <div class="options">
        <button
          class="option"
          class:selected={chosenMode === "keychain"}
          onclick={() => (chosenMode = "keychain")}
        >
          <span class="option-icon"><KeyRound size={18} /></span>
          <span class="option-body">
            <strong>OS keychain</strong>
            <span class="hint">Recommended. The vault opens without a password; the key
            lives in your system keyring.</span>
          </span>
          {#if chosenMode === "keychain"}<Check size={16} class="tick" />{/if}
        </button>
        <button
          class="option"
          class:selected={chosenMode === "passphrase"}
          onclick={() => (chosenMode = "passphrase")}
        >
          <span class="option-icon"><Lock size={18} /></span>
          <span class="option-body">
            <strong>Passphrase</strong>
            <span class="hint">Paranoid mode. You enter a passphrase every launch; the
            key is derived from it with argon2id.</span>
          </span>
          {#if chosenMode === "passphrase"}<Check size={16} class="tick" />{/if}
        </button>
      </div>

      {#if chosenMode === "passphrase"}
        <div class="pass-row">
          <input
            type="password"
            bind:value={passphrase}
            placeholder="Passphrase (8+ characters)"
          />
          <input
            type="password"
            bind:value={passphraseConfirm}
            placeholder="Repeat passphrase"
          />
        </div>
      {/if}

      {#if error}<p class="error-text">{error}</p>{/if}

      <footer>
        <button class="btn primary" disabled={busy} onclick={setupVault}>
          {busy ? "Creating vault..." : "Create vault"}
          <ArrowRight size={15} />
        </button>
      </footer>
    {:else if step === "import"}
      {#if candidates}
        <p class="step-title">
          <FileUp size={15} />
          Found {candidates.length} connection{candidates.length === 1 ? "" : "s"} in ~/.ssh/config
        </p>
        <div class="candidate-list">
          {#each candidates as candidate, i (i)}
            <label class="candidate" class:duplicate={candidate.duplicate_of !== null}>
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
              {#if candidate.duplicate_of !== null}
                <span class="dup">already exists</span>
              {/if}
            </label>
          {/each}
        </div>
      {:else}
        <p class="step-title">No ~/.ssh/config found — you can import it later any time.</p>
      {/if}
      <footer>
        <button class="btn ghost" onclick={() => (step = "keys")}>
          {candidates ? "Skip" : "Continue"}
        </button>
        {#if candidates}
          <button class="btn primary" disabled={busy || selected.size === 0} onclick={commitImport}>
            Import {selected.size} selected
          </button>
        {/if}
      </footer>
    {:else if step === "keys"}
      <p class="step-title">
        <KeyRound size={15} />
        Generate an SSH key? You can also add keys later.
      </p>
      <div class="pass-row">
        <input class="mono" bind:value={keyName} placeholder="Key file name" spellcheck="false" />
        <input
          type="password"
          bind:value={keyPassphrase}
          placeholder="Passphrase (optional)"
        />
      </div>
      {#if error}<p class="error-text">{error}</p>{/if}
      <footer>
        <button class="btn ghost" onclick={() => (step = "done")}>Skip</button>
        <button class="btn primary" disabled={busy} onclick={generateKey}>
          {busy ? "Generating..." : "Generate ed25519 key"}
        </button>
      </footer>
    {:else if step === "done"}
      <p class="step-title">
        <Server size={15} />
        You are set. Add a connection from the sidebar or import your ssh
        config any time.
      </p>
      <footer>
        <button class="btn primary" onclick={finish}>
          Start using Gapura
          <ArrowRight size={15} />
        </button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .wizard {
    flex: 1;
    display: grid;
    place-items: center;
    background: var(--bg);
    padding: 24px;
  }

  .card {
    width: 520px;
    max-width: 100%;
    max-height: 86vh;
    overflow-y: auto;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 22px 24px;
  }

  header {
    margin-bottom: 18px;
  }

  .mark {
    display: inline-grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: var(--accent-soft);
    color: var(--accent);
    margin-bottom: 10px;
  }

  h1 {
    font-size: 19px;
    margin-bottom: 4px;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .option {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    text-align: left;
    padding: 12px 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--bg-input);
    transition: border-color 0.12s, background 0.12s;
  }

  .option:hover {
    border-color: var(--border-strong);
  }

  .option.selected {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .option-icon {
    color: var(--text-muted);
    margin-top: 1px;
  }

  .option.selected .option-icon {
    color: var(--accent);
  }

  .option-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .option :global(.tick) {
    color: var(--accent);
    margin-top: 2px;
  }

  .pass-row {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .pass-row input {
    flex: 1;
  }

  .step-title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
    margin: 4px 0 12px;
  }

  .candidate-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 260px;
    overflow-y: auto;
    margin-bottom: 6px;
  }

  .candidate {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 10px;
    border-radius: 6px;
    border: 1px solid var(--border);
    cursor: pointer;
  }

  .candidate:hover {
    background: var(--bg-hover);
  }

  .candidate.duplicate {
    opacity: 0.65;
  }

  .c-name {
    font-weight: 600;
    min-width: 110px;
  }

  .c-host {
    flex: 1;
    font-size: 12px;
    color: var(--text-muted);
  }

  .dup {
    font-size: 11px;
    color: var(--warning);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
