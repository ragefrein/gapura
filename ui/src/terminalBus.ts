// Per-session terminal writers. The app-level event listener routes
// session:data payloads here; each TerminalView registers itself.

const writers = new Map<string, (data: Uint8Array) => void>();

export function registerWriter(sessionId: string, write: (data: Uint8Array) => void) {
  writers.set(sessionId, write);
}

export function unregisterWriter(sessionId: string) {
  writers.delete(sessionId);
}

export function writeToTerminal(sessionId: string, data: Uint8Array): boolean {
  const writer = writers.get(sessionId);
  if (!writer) return false;
  writer(data);
  return true;
}
