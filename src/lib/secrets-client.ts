/**
 * Client helpers for secret persistence.
 * Desktop: Electron safeStorage via IPC.
 * Browser preview: sessionStorage only (never localStorage for long-lived secrets).
 */

export type SecretKey = "apiKey" | "oauth" | "ssoCookie" | "githubToken";

function electronSecrets() {
  return typeof window !== "undefined"
    ? window.grokhubDesktop?.secrets
    : undefined;
}

export async function secretsSet(key: SecretKey, value: string): Promise<void> {
  const e = electronSecrets();
  if (e?.set) {
    await e.set(key, value);
    return;
  }
  try {
    if (value) sessionStorage.setItem(`grokhub.secret.${key}`, value);
    else sessionStorage.removeItem(`grokhub.secret.${key}`);
  } catch {
    /* private mode */
  }
}

export async function secretsGet(key: SecretKey): Promise<string> {
  const e = electronSecrets();
  if (e?.get) {
    const r = await e.get(key);
    return r?.value || "";
  }
  try {
    return sessionStorage.getItem(`grokhub.secret.${key}`) || "";
  } catch {
    return "";
  }
}

export async function secretsDelete(key: SecretKey): Promise<void> {
  const e = electronSecrets();
  if (e?.delete) {
    await e.delete(key);
    return;
  }
  try {
    sessionStorage.removeItem(`grokhub.secret.${key}`);
  } catch {
    /* ignore */
  }
}

export async function loadAllSecrets(): Promise<Partial<Record<SecretKey, string>>> {
  const keys: SecretKey[] = ["apiKey", "oauth", "ssoCookie", "githubToken"];
  const out: Partial<Record<SecretKey, string>> = {};
  await Promise.all(
    keys.map(async (k) => {
      const v = await secretsGet(k);
      if (v) out[k] = v;
    }),
  );
  return out;
}
