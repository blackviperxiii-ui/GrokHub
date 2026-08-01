//#region node_modules/.nitro/vite/services/ssr/assets/grok-client-DNfOOpUN.js
async function rpc(path, action, body = {}, init) {
	const res = await fetch(path, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({
			action,
			...body
		}),
		signal: init?.signal
	});
	const data = await res.json().catch(() => ({}));
	if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
	return data;
}
async function grokChat(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.chat) return desktop.chat(opts);
	return rpc("/api/grok", "chat", opts, { signal: opts.signal });
}
/**
* Stream chat tokens. Prefer Electron IPC stream; fall back to SSE /api/grok,
* then non-stream chat.
*/
async function grokChatStream(opts, handlers) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.chatStream) return desktop.chatStream(opts, handlers);
	try {
		const res = await fetch("/api/grok", {
			method: "POST",
			headers: {
				"content-type": "application/json",
				accept: "text/event-stream"
			},
			body: JSON.stringify({
				action: "chatStream",
				...opts
			}),
			signal: handlers.signal
		});
		const ctype = res.headers.get("content-type") || "";
		if (res.ok && (ctype.includes("text/event-stream") || ctype.includes("ndjson"))) {
			handlers.onStatus?.("streaming");
			const reader = res.body?.getReader();
			if (!reader) throw new Error("no stream body");
			const decoder = new TextDecoder();
			let buffer = "";
			let content = "";
			let model;
			let tokens;
			let usage;
			let rateLimit;
			while (true) {
				if (handlers.signal?.aborted) {
					try {
						await reader.cancel();
					} catch {}
					return {
						ok: false,
						aborted: true,
						error: "Stopped",
						content,
						model,
						usage,
						rateLimit
					};
				}
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true });
				const parts = buffer.split("\n");
				buffer = parts.pop() || "";
				for (const raw of parts) {
					const line = raw.trim();
					if (!line || line.startsWith(":")) continue;
					let payload = line;
					if (line.startsWith("data:")) payload = line.slice(5).trim();
					if (payload === "[DONE]") continue;
					try {
						const evt = JSON.parse(payload);
						if (evt.type === "delta" && evt.delta) {
							content += evt.delta;
							handlers.onDelta(evt.delta);
						} else if (evt.type === "status" && evt.content) handlers.onStatus?.(evt.content);
						else if (evt.type === "done") {
							model = evt.model || model;
							tokens = evt.tokens || tokens;
							usage = evt.usage || usage;
							rateLimit = evt.rateLimit || rateLimit;
							if (evt.content && !content) {
								content = evt.content;
								handlers.onDelta(evt.content);
							}
						} else if (evt.type === "error") return {
							ok: false,
							error: evt.error || "stream error",
							content,
							model,
							tokens,
							usage,
							rateLimit
						};
						else if (evt.delta) {
							content += evt.delta;
							handlers.onDelta(evt.delta);
						}
					} catch {}
				}
			}
			if (!content.trim()) return {
				ok: false,
				error: "Empty stream",
				model,
				tokens,
				usage,
				rateLimit
			};
			return {
				ok: true,
				content,
				model,
				tokens,
				usage,
				rateLimit
			};
		}
		if (res.ok) {
			const data = await res.json();
			if (data.ok && data.content) handlers.onDelta(data.content);
			return data;
		}
	} catch (e) {
		if (handlers.signal?.aborted || e instanceof Error && e.name === "AbortError") return {
			ok: false,
			aborted: true,
			error: "Stopped"
		};
	}
	handlers.onStatus?.("fallback");
	const full = await grokChat({
		...opts,
		signal: handlers.signal
	});
	if (full.ok && full.content) handlers.onDelta(full.content);
	return full;
}
async function grokProbe(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.probe) return desktop.probe(opts?.apiKey, opts?.accessToken);
	return rpc("/api/grok", "probe", {
		apiKey: opts?.apiKey || "",
		accessToken: opts?.accessToken || ""
	});
}
async function oauthStart() {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.oauthStart) return desktop.oauthStart();
	return rpc("/api/grok", "oauthStart", {});
}
async function oauthPoll(deviceCode) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.oauthPoll) return desktop.oauthPoll(deviceCode);
	return rpc("/api/grok", "oauthPoll", { deviceCode });
}
async function oauthEnsure(tokens) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.oauthEnsure) return desktop.oauthEnsure(tokens);
	return rpc("/api/grok", "oauthEnsure", { tokens });
}
async function grokImagine(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.imagine) return desktop.imagine(opts);
	return rpc("/api/grok", "imagine", opts);
}
async function checkUpdate(token) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.checkUpdate) return desktop.checkUpdate({ token });
	return rpc("/api/update", "check", { token: token || "" });
}
async function applyUpdate(token, force = true) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.applyUpdate) return desktop.applyUpdate({
		token,
		force,
		restart: true
	});
	return rpc("/api/update", "apply", {
		token: token || "",
		force,
		restart: false
	});
}
//#endregion
export { applyUpdate, checkUpdate, grokChatStream, grokImagine, grokProbe, oauthEnsure, oauthPoll, oauthStart };
