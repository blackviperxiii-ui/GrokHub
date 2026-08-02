//#region node_modules/.nitro/vite/services/ssr/assets/setup-crypto-bPN_Pa4r.js
/**
* Optional passphrase protection for setup packs (export / Gist payload).
* WebCrypto AES-GCM + PBKDF2 — browser & Electron renderer.
*/
var ITERATIONS = 12e4;
var ENC_KIND = "grokhub-setup-enc-v1";
function b64(buf) {
	const bytes = new Uint8Array(buf);
	let s = "";
	for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
	return btoa(s);
}
function fromB64(s) {
	const bin = atob(s);
	const out = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
	return out;
}
async function deriveKey(passphrase, salt) {
	const enc = new TextEncoder();
	const base = await crypto.subtle.importKey("raw", enc.encode(passphrase), "PBKDF2", false, ["deriveKey"]);
	return crypto.subtle.deriveKey({
		name: "PBKDF2",
		salt,
		iterations: ITERATIONS,
		hash: "SHA-256"
	}, base, {
		name: "AES-GCM",
		length: 256
	}, false, ["encrypt", "decrypt"]);
}
function isEncryptedSetupBlob(raw) {
	return !!raw && typeof raw === "object" && raw.kind === ENC_KIND && typeof raw.ct === "string";
}
async function encryptSetupJson(plainJson, passphrase) {
	if (!passphrase.trim()) throw new Error("Passphrase required");
	const salt = crypto.getRandomValues(/* @__PURE__ */ new Uint8Array(16));
	const iv = crypto.getRandomValues(/* @__PURE__ */ new Uint8Array(12));
	const key = await deriveKey(passphrase, salt);
	const ct = await crypto.subtle.encrypt({
		name: "AES-GCM",
		iv
	}, key, new TextEncoder().encode(plainJson));
	return {
		kind: ENC_KIND,
		v: 1,
		salt: b64(salt.buffer),
		iv: b64(iv.buffer),
		ct: b64(ct)
	};
}
async function decryptSetupJson(blob, passphrase) {
	if (!passphrase.trim()) throw new Error("Passphrase required");
	const salt = fromB64(blob.salt);
	const iv = fromB64(blob.iv);
	const ct = fromB64(blob.ct);
	const key = await deriveKey(passphrase, salt);
	const plain = await crypto.subtle.decrypt({
		name: "AES-GCM",
		iv
	}, key, ct);
	return new TextDecoder().decode(plain);
}
/** Parse plain pack or decrypt encrypted blob → JSON string of SetupPack */
async function unwrapSetupPayload(text, passphrase) {
	const trimmed = text.trim();
	let parsed;
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
//#endregion
export { encryptSetupJson, unwrapSetupPayload };
