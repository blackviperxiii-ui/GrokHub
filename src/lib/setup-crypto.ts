/**
 * Optional passphrase protection for setup packs (export / Gist payload).
 * WebCrypto AES-GCM + PBKDF2 — browser & Electron renderer.
 */

const ITERATIONS = 120_000;
const ENC_KIND = "grokhub-setup-enc-v1";

export type EncryptedSetupBlob = {
  kind: typeof ENC_KIND;
  v: 1;
  salt: string;
  iv: string;
  ct: string;
};

function b64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]!);
  return btoa(s);
}

function fromB64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function deriveKey(passphrase: string, salt: Uint8Array): Promise<CryptoKey> {
  const enc = new TextEncoder();
  const base = await crypto.subtle.importKey(
    "raw",
    enc.encode(passphrase),
    "PBKDF2",
    false,
    ["deriveKey"],
  );
  return crypto.subtle.deriveKey(
    {
      name: "PBKDF2",
      salt: salt as BufferSource,
      iterations: ITERATIONS,
      hash: "SHA-256",
    },
    base,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

export function isEncryptedSetupBlob(raw: unknown): raw is EncryptedSetupBlob {
  return (
    !!raw &&
    typeof raw === "object" &&
    (raw as EncryptedSetupBlob).kind === ENC_KIND &&
    typeof (raw as EncryptedSetupBlob).ct === "string"
  );
}

export async function encryptSetupJson(
  plainJson: string,
  passphrase: string,
): Promise<EncryptedSetupBlob> {
  if (!passphrase.trim()) throw new Error("Passphrase required");
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(passphrase, salt);
  const ct = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv },
    key,
    new TextEncoder().encode(plainJson),
  );
  return {
    kind: ENC_KIND,
    v: 1,
    salt: b64(salt.buffer),
    iv: b64(iv.buffer),
    ct: b64(ct),
  };
}

export async function decryptSetupJson(
  blob: EncryptedSetupBlob,
  passphrase: string,
): Promise<string> {
  if (!passphrase.trim()) throw new Error("Passphrase required");
  const salt = fromB64(blob.salt);
  const iv = fromB64(blob.iv);
  const ct = fromB64(blob.ct);
  const key = await deriveKey(passphrase, salt);
  const plain = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: iv as BufferSource },
    key,
    ct as BufferSource,
  );
  return new TextDecoder().decode(plain);
}

/** Parse plain pack or decrypt encrypted blob → JSON string of SetupPack */
export async function unwrapSetupPayload(
  text: string,
  passphrase?: string,
): Promise<string> {
  const trimmed = text.trim();
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    throw new Error("Not valid JSON");
  }
  if (isEncryptedSetupBlob(parsed)) {
    if (!passphrase?.trim()) throw new Error("This pack is encrypted — enter the passphrase");
    return decryptSetupJson(parsed, passphrase);
  }
  return trimmed;
}
