/**
 * The worker session: `{ token, name }` kept in `sessionStorage`, so it dies with the tab.
 * Shaped for `useSyncExternalStore`: `getSession()` returns the same object until it changes.
 */
export type Session = { token: string; name: string };

const STORAGE_KEY = "nullbox.session";
const listeners = new Set<() => void>();

function load(): Session | null {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Session>;
    if (typeof parsed.token === "string" && typeof parsed.name === "string") {
      return { token: parsed.token, name: parsed.name };
    }
  } catch {
    // unavailable or corrupt: treat as logged out
  }
  return null;
}

let current: Session | null = load();

export function getSession(): Session | null {
  return current;
}

function emit(): void {
  for (const listener of listeners) listener();
}

export function setSession(session: Session): void {
  current = { token: session.token, name: session.name };
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    // keep it in memory only
  }
  emit();
}

export function clearSession(): void {
  if (current === null) return;
  current = null;
  try {
    sessionStorage.removeItem(STORAGE_KEY);
  } catch {
    // nothing to clean up
  }
  emit();
}

export function subscribeSession(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
