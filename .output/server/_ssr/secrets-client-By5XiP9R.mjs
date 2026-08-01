//#region node_modules/.nitro/vite/services/ssr/assets/secrets-client-By5XiP9R.js
function electronSecrets() {
	return typeof window !== "undefined" ? window.grokhubDesktop?.secrets : void 0;
}
async function secretsSet(key, value) {
	const e = electronSecrets();
	if (e?.set) {
		await e.set(key, value);
		return;
	}
	try {
		if (value) sessionStorage.setItem(`grokhub.secret.${key}`, value);
		else sessionStorage.removeItem(`grokhub.secret.${key}`);
	} catch {}
}
async function secretsGet(key) {
	const e = electronSecrets();
	if (e?.get) return (await e.get(key))?.value || "";
	try {
		return sessionStorage.getItem(`grokhub.secret.${key}`) || "";
	} catch {
		return "";
	}
}
async function loadAllSecrets() {
	const keys = [
		"apiKey",
		"oauth",
		"ssoCookie",
		"githubToken"
	];
	const out = {};
	await Promise.all(keys.map(async (k) => {
		const v = await secretsGet(k);
		if (v) out[k] = v;
	}));
	return out;
}
//#endregion
export { loadAllSecrets, secretsSet };
