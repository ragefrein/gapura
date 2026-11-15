// Typed wrappers around Tauri invoke() and event listeners.
// Argument names are camelCase here; Tauri maps them to the snake_case
// parameters of the Rust commands.

import { invoke } from "@tauri-apps/api/core";

export type Theme = "dark" | "light";

export type AuthMethod =
  | { kind: "agent" }
  | { kind: "password"; data: { secret_id: string } }
  | {
      kind: "key_file";
      data: { path: string; key_id: string | null; passphrase_secret_id: string | null };
    };

export interface Connection {
  id: string;
  name: string;
  group_id: string | null;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  tags: string[];
  color: string | null;
  jump_host_id: string | null;
  notes: string;
  created_at: string;
  updated_at: string;
}

export interface Group {
  id: string;
  name: string;
  created_at: string;
}

export interface SshKey {
  id: string;
  name: string;
  path: string;
  public_key: string;
  fingerprint: string;
  has_passphrase: boolean;
  created_at: string;
}

export interface Settings {
  theme: Theme;
  terminal_font_family: string;
  terminal_font_size: number;
  scrollback_lines: number;
}

export interface KnownHostEntry {
  host_port: string;
  fingerprint: string;
  algorithm: string;
  added_at: string;
}

export interface ImportCandidate {
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  notes: string;
  duplicate_of: string | null;
  warnings: string[];
}

export interface VaultStatus {
  exists: boolean;
  unlocked: boolean;
  mode: "keychain" | "passphrase" | null;
  keychain_error: string | null;
}

export interface SessionInfo {
  id: string;
  connection_id: string;
  fingerprint: string;
  new_host_key: boolean;
}

export interface ConnectionInput {
  id: string | null;
  name: string;
  group_id: string | null;
  host: string;
  port: number;
  username: string;
  auth:
    | { kind: "agent" }
    | { kind: "password"; data: { password: string | null } }
    | { kind: "key_file"; data: { path: string; passphrase: string | null } };
  tags: string[];
  color: string | null;
  notes: string;
  jump_host_id: string | null;
}

export type ForwardKind = "local" | "remote";

export interface ForwardRule {
  id: string;
  connection_id: string;
  kind: ForwardKind;
  bind_host: string;
  bind_port: number;
  target_host: string;
  target_port: number;
  enabled: boolean;
  auto_start: boolean;
  created_at: string;
}

export interface ActiveForward {
  rule_id: string;
  session_id: string;
  kind: ForwardKind;
  bind_host: string;
  bind_port: number;
  target_host: string;
  target_port: number;
}

export interface Snippet {
  id: string;
  name: string;
  command: string;
  connection_id: string | null;
  created_at: string;
}

export interface SshCommandDraft {
  username: string | null;
  host: string;
  port: number | null;
  identity_path: string | null;
  warnings: string[];
}

export interface ConnectionStatus {
  connection_id: string;
  up: boolean;
  latency_ms: number | null;
}

export interface AppPaths {
  data_dir: string;
  config_dir: string;
}

/** Error shape shipped by GapuraError's serde impl (see core error.rs). */
export interface IpcError {
  code: string;
  message: string;
  host?: string;
  port?: number;
  stored_fingerprint?: string;
  actual_fingerprint?: string;
}

export function isIpcError(value: unknown): value is IpcError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as IpcError).code === "string" &&
    typeof (value as IpcError).message === "string"
  );
}

export const api = {
  vaultStatus: () => invoke<VaultStatus>("vault_status"),
  vaultSetup: (mode: "keychain" | "passphrase", passphrase?: string) =>
    invoke<void>("vault_setup", { mode, passphrase: passphrase ?? null }),
  vaultUnlock: (passphrase?: string) =>
    invoke<void>("vault_unlock", { passphrase: passphrase ?? null }),
  vaultLock: () => invoke<void>("vault_lock"),

  listConnections: () => invoke<Connection[]>("list_connections"),
  saveConnection: (input: ConnectionInput) => invoke<Connection>("save_connection", { input }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),

  listGroups: () => invoke<Group[]>("list_groups"),
  saveGroup: (id: string | null, name: string) => invoke<Group>("save_group", { id, name }),
  deleteGroup: (id: string) => invoke<void>("delete_group", { id }),

  sessionOpen: (connectionId: string, cols: number, rows: number) =>
    invoke<SessionInfo>("session_open", { connectionId, cols, rows }),
  sessionReconnect: (sessionId: string, cols: number, rows: number) =>
    invoke<SessionInfo>("session_reconnect", { sessionId, cols, rows }),
  sessionWrite: (sessionId: string, data: string) =>
    invoke<void>("session_write", { sessionId, data }),
  sessionResize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("session_resize", { sessionId, cols, rows }),
  sessionClose: (sessionId: string) => invoke<void>("session_close", { sessionId }),

  listKeys: () => invoke<SshKey[]>("list_keys"),
  generateKey: (name: string, passphrase?: string) =>
    invoke<SshKey>("generate_key", { input: { name, passphrase: passphrase ?? null } }),
  deleteKey: (id: string) => invoke<void>("delete_key", { id }),

  importSshConfig: (path?: string) =>
    invoke<ImportCandidate[]>("import_ssh_config", { path: path ?? null }),
  commitImport: (candidates: ImportCandidate[]) =>
    invoke<number>("commit_import", { input: { candidates } }),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),

  listKnownHosts: () => invoke<KnownHostEntry[]>("list_known_hosts"),
  removeKnownHost: (hostPort: string) => invoke<boolean>("remove_known_host", { hostPort }),

  listForwardRules: (connectionId?: string) =>
    invoke<ForwardRule[]>("list_forward_rules", { connectionId: connectionId ?? null }),
  saveForwardRule: (input: Omit<ForwardRule, "id" | "created_at"> & { id?: string | null }) =>
    invoke<ForwardRule>("save_forward_rule", { input }),
  deleteForwardRule: (id: string) => invoke<void>("delete_forward_rule", { id }),
  listActiveForwards: () => invoke<ActiveForward[]>("list_active_forwards"),
  startForward: (sessionId: string, ruleId: string) =>
    invoke<void>("start_forward", { sessionId, ruleId }),
  stopForward: (ruleId: string) => invoke<void>("stop_forward", { ruleId }),

  listSnippets: (connectionId?: string) =>
    invoke<Snippet[]>("list_snippets", { connectionId: connectionId ?? null }),
  saveSnippet: (input: {
    id?: string | null;
    name: string;
    command: string;
    connection_id?: string | null;
  }) => invoke<Snippet>("save_snippet", { input }),
  deleteSnippet: (id: string) => invoke<void>("delete_snippet", { id }),

  checkConnectionStatus: (ids: string[]) =>
    invoke<ConnectionStatus[]>("check_connection_status", { ids }),
  parseSshCommand: (text: string) =>
    invoke<SshCommandDraft>("parse_ssh_command_text", { text }),

  appPaths: () => invoke<AppPaths>("app_paths"),
  openDataDir: () => invoke<void>("open_data_dir"),
  versionInfo: () => invoke<{ version: string }>("version_info"),
};

export { listen } from "@tauri-apps/api/event";
