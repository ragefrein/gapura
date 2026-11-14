# Gapura

A local-first SSH connection manager for Linux and Windows.

Gapura keeps your servers, keys, and passwords in an encrypted vault on your own
machine. The vault is AES-256-GCM encrypted, and the master key lives in your OS
keychain — or behind a passphrase, if you prefer that. No accounts, no cloud, no
telemetry. The only network traffic is the SSH connections you make yourself.

## Why

Managing a handful of servers usually falls into one of these:

- `~/.ssh/config` edited by hand. It works, but there is no search, no notes,
  no place for passwords, and no status.
- Subscription-based managers that want your credentials in their cloud.
- Electron-based apps, which spend a few hundred megabytes of RAM to show a
  terminal.

Gapura is a small native desktop app: a Rust core, a Tauri 2 shell, and an
embedded terminal rendered by xterm.js. It is free and open source, and it is
meant to stay that way.

## Features

- Connection list with folders, tags, colors, notes, and fuzzy search (Ctrl K)
- Password, key file, and SSH agent authentication
- Embedded terminal: scrollback, output search, copy/paste, live resize
- Multiple sessions in tabs, with a reconnect banner when a session drops
- Known-hosts TOFU: the first connect pins the server fingerprint; any change
  is blocked with a clear warning instead of being silently accepted
- Encrypted vault (argon2id + AES-256-GCM), unlocked from the OS keychain or
  with a passphrase
- One-way import from `~/.ssh/config`, reviewed before anything is saved
- ed25519 key generation with fingerprints
- Dark and light themes

## Status

v0.1 is in active development. Expect rough edges. SFTP browsing, port
forwarding, jump hosts, and snippets are planned for v0.2.

## Why the name

*Gapura* is the ceremonial gateway at the entrance of Indonesian temples —
the gate you pass through to go in. That is what this app is for: every server
in the list is a door, and Gapura is how you open it.

## Building from source

You need a stable Rust toolchain and Node 20 or newer. On Linux, the usual
Tauri system libraries are required (`libwebkit2gtk-4.1-dev` and friends).

```sh
cd ui
npm install

# then, from the repository root (the Tauri CLI looks for src-tauri
# in the current folder and below, so run it from here, not from ui/):
npx --prefix ui tauri dev    # run the app in development mode
npx --prefix ui tauri build  # produce installers
```

Rust unit and integration tests:

```sh
cargo test -p gapura-core   # core: vault, parser, sshd fixtures
cargo test --workspace      # everything, incl. the command-layer E2E
```

## Security notes

- The vault file on disk contains only ciphertext. Passwords and key
  passphrases are never written in plain text.
- Private keys are referenced by path, never copied into the vault.
- Server fingerprints are stored unencrypted (like OpenSSH known_hosts) —
  that data is public by nature.
- Error types are built to never carry credentials, and logs are written
  without secrets.

## License

Dual-licensed under MIT or Apache-2.0, at your option.
