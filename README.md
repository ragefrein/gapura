# Gapura

> SSH connection manager — fast, native, **local-first**. For Linux & Windows.

Gapura is an open-source SSH connection manager built in Rust (Tauri 2):

- 🔐 **Local-first vault** — credentials encrypted (AES-256-GCM) on your machine, master key in the OS keychain. No cloud, no account.
- ⚡ **Native & light** — Tauri 2, not Electron.
- 🖥️ **Embedded terminal** — xterm.js under the hood; vim, htop and tmux just work.
- 📥 **One-minute onboarding** — import your existing `~/.ssh/config`.

## Status

🚧 **Planning phase** — not usable yet. Current milestone: **M0 spike** (see [PLAN](docs/PLAN.md)).

## Documentation

| Doc | Contents |
|---|---|
| [PRD](docs/PRD.md) | Problem, personas, feature set (MVP frozen), non-goals, metrics |
| [design](docs/design.md) | Architecture, vault crypto, SSH layer, open decisions D1–D5 |
| [PLAN](docs/PLAN.md) | Milestones M0–M5, timeline, risks, working agreements |

## Why "Gapura"?

*Gapura* is the ceremonial gateway of Indonesian temples — the gate you pass through to enter. Every server behind this app is just that: a door, and Gapura is your way in.

---

License: TBD (open question Q2 in the PRD).
