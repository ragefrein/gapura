// Per-session terminal writers. The app-level event listener routes
// session:data payloads here; each TerminalView registers itself.
//
// The actor starts emitting the remote banner within milliseconds of the
// session opening — often before the webview has finished mounting the
// TerminalView for the new tab. Anything that arrives without a writer is
// queued here and flushed on registration, so the first screenful of a
// session is never lost.

const MAX_PENDING_BYTES = 1024 * 1024;

const writers = new Map<string, (data: Uint8Array) => void>();
const pending = new Map<string, Uint8Array[]>();

export function registerWriter(sessionId: string, write: (data: Uint8Array) => void) {
  writers.set(sessionId, write);
  const queued = pending.get(sessionId);
  if (queued) {
    pending.delete(sessionId);
    for (const chunk of queued) write(chunk);
  }
}

export function unregisterWriter(sessionId: string) {
  writers.delete(sessionId);
  pending.delete(sessionId);
}

export function writeToTerminal(sessionId: string, data: Uint8Array): boolean {
  const writer = writers.get(sessionId);
  if (writer) {
    writer(data);
    return true;
  }
  // Writer not mounted yet: queue until it is.
  let queue = pending.get(sessionId);
  if (!queue) {
    queue = [];
    pending.set(sessionId, queue);
  }
  queue.push(data);
  const total = queue.reduce((sum, chunk) => sum + chunk.length, 0);
  if (total > MAX_PENDING_BYTES) {
    // Drop the oldest half rather than growing without bound.
    const keep = queue.slice(Math.floor(queue.length / 2));
    pending.set(sessionId, keep);
  }
  return false;
}
