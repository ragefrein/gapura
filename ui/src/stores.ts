// Central app state. Small enough that plain Svelte stores stay clearer
// than a rune-based class.

import { writable } from "svelte/store";
import type { Connection, Group, Settings, VaultStatus } from "./ipc";

export const vaultStatus = writable<VaultStatus>({
  exists: false,
  unlocked: false,
  mode: null,
  keychain_error: null,
});

export const connections = writable<Connection[]>([]);
export const groups = writable<Group[]>([]);
export const settings = writable<Settings>({
  theme: "dark",
  terminal_font_family: "JetBrains Mono",
  terminal_font_size: 14,
  scrollback_lines: 10_000,
});

export type ViewName = "terminal" | "keys" | "settings";

export const activeView = writable<ViewName>("terminal");

export interface SessionTab {
  id: string;
  connectionId: string;
  name: string;
  color: string | null;
  state: "connecting" | "connected" | "closed";
  closedReason?: string;
}

export const sessions = writable<SessionTab[]>([]);
export const activeSessionId = writable<string | null>(null);

export interface Toast {
  id: number;
  kind: "info" | "error";
  message: string;
}

export const toasts = writable<Toast[]>([]);

let toastSeq = 0;

export function pushToast(kind: Toast["kind"], message: string) {
  const id = ++toastSeq;
  toasts.update((list) => [...list, { id, kind, message }]);
  setTimeout(() => {
    toasts.update((list) => list.filter((t) => t.id !== id));
  }, 4200);
}

export function dismissToast(id: number) {
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function markSessionState(id: string, state: SessionTab["state"], reason?: string) {
  sessions.update((list) =>
    list.map((s) => (s.id === id ? { ...s, state, closedReason: reason } : s)),
  );
}
