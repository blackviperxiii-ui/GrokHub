import { g as modelIdForMode, v as parseRateLimitHeaders, y as resolveMode } from "./version-BE4o4tL_.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/grok-BlMJRCyx.js
var XAI_BASE = "https://api.x.ai/v1";
/** Map GrokHub modes → xAI model IDs */
function modelForMode(mode, prompt = "") {
	return modelIdForMode(mode, prompt);
}
function systemPromptForMode(mode, prompt = "") {
	const base = `You are Grok, running inside GrokHub (a desktop agent control plane on the user's Linux machine).
Help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
Do not prefix replies with mode labels like [Fast] or [Auto → …]. Just answer.

You have unsandboxed host access when the desktop gateway is connected.
When you need real filesystem / shell data (Downloads, home, processes, etc.), do NOT invent results.
Put the host command on its OWN line, alone, like:
HOST_CMD: ls -la "$HOME/Downloads"
Never glue HOST_CMD onto a prose sentence. Prefer one simple command (ls, head, cat, find, stat).
The runtime executes it and returns HOST_RESULT — then summarize clearly for the user.
You may use multiple HOST_CMD rounds if needed.`;
	switch (resolveMode(mode, prompt)) {
		case "fast": return `${base}\nMode: Fast — concise answers, minimal preamble.`;
		case "expert": return `${base}\nMode: Expert — reason carefully, surface tradeoffs, cite assumptions.`;
		case "heavy": return `${base}\nMode: Heavy (team of experts) — consider multiple angles (ops, research, build, critique), then synthesize a clear recommendation.`;
		case "build": return `${base}\nMode: Build — prioritize working code, file paths, and implementable steps. Prefer complete snippets.`;
		default: return base;
	}
}
function resolveBearer(req) {
	if (req.accessToken?.trim()) return {
		bearer: req.accessToken.trim(),
		source: "oauth"
	};
	if (req.apiKey?.trim()) return {
		bearer: req.apiKey.trim(),
		source: "key"
	};
	const env = process.env.XAI_API_KEY?.trim() || process.env.GROK_API_KEY?.trim() || "";
	if (env) return {
		bearer: env,
		source: "env"
	};
	return null;
}
function buildBody(req, stream) {
	const mode = req.mode ?? "auto";
	const lastUser = [...req.messages].reverse().find((m) => m.role === "user")?.content ?? "";
	const routed = resolveMode(mode, lastUser);
	const model = req.model || modelForMode(mode, lastUser);
	return {
		model,
		body: {
			model,
			messages: [{
				role: "system",
				content: systemPromptForMode(mode, lastUser) + (req.workspaceContext?.trim() ? `\n\n## Imported OpenClaw workspace context\n${req.workspaceContext.trim().slice(0, 24e3)}` : "")
			}, ...req.messages.filter((m) => m.role !== "system")],
			temperature: req.temperature ?? (routed === "fast" ? .5 : routed === "build" ? .4 : routed === "heavy" ? .8 : .7),
			max_tokens: req.maxTokens ?? (routed === "heavy" ? 4096 : routed === "build" ? 8192 : routed === "expert" ? 3072 : 2048),
			stream
		},
		routed
	};
}
async function callXaiChat(req) {
	const auth = resolveBearer(req);
	if (!auth) return {
		ok: false,
		status: 401,
		error: "Not connected to Grok. Use Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key."
	};
	const { model, body } = buildBody(req, false);
	try {
		const res = await fetch(`${XAI_BASE}/chat/completions`, {
			method: "POST",
			headers: {
				"content-type": "application/json",
				authorization: `Bearer ${auth.bearer}`
			},
			body: JSON.stringify(body),
			signal: req.signal
		});
		const data = await res.json().catch(() => ({}));
		const rateLimit = parseRateLimitHeaders(res.headers);
		if (!res.ok) {
			if (res.status === 404 || typeof data.error === "object" && /model|not found|invalid/i.test(data.error?.message || "")) {
				if (model === "grok-4.5" || model === "grok-4-5") return callXaiChat({
					...req,
					model: "grok-4.3"
				});
				if (model === "grok-4.3") return callXaiChat({
					...req,
					model: "grok-4"
				});
				if (model === "grok-code-fast-1" || /build/i.test(model)) return callXaiChat({
					...req,
					model: "grok-code-fast-1"
				});
				if (model === "grok-4-1-fast-non-reasoning") return callXaiChat({
					...req,
					model: "grok-3-mini-fast"
				});
			}
			const msg = typeof data.error === "string" ? data.error : data.error?.message || `xAI error ${res.status}`;
			return {
				ok: false,
				status: res.status,
				error: msg,
				model,
				rateLimit
			};
		}
		const content = data.choices?.[0]?.message?.content?.trim();
		if (!content) return {
			ok: false,
			status: res.status,
			error: "Empty response from Grok",
			model,
			rateLimit
		};
		return {
			ok: true,
			content,
			model: data.model || model,
			usage: data.usage,
			status: res.status,
			rateLimit
		};
	} catch (e) {
		if (req.signal?.aborted || e instanceof Error && e.name === "AbortError") return {
			ok: false,
			aborted: true,
			error: "Stopped"
		};
		return {
			ok: false,
			error: e instanceof Error ? e.message : "Network error calling xAI"
		};
	}
}
/** Stream Grok tokens (SSE). Calls onDelta for each piece of content. */
async function callXaiChatStream(req, handlers = {}) {
	const auth = resolveBearer(req);
	if (!auth) return {
		ok: false,
		status: 401,
		error: "Not connected to Grok. Use Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key."
	};
	const signal = handlers.signal || req.signal;
	const { model, body } = buildBody(req, true);
	handlers.onStatus?.("connecting");
	try {
		const res = await fetch(`${XAI_BASE}/chat/completions`, {
			method: "POST",
			headers: {
				"content-type": "application/json",
				authorization: `Bearer ${auth.bearer}`,
				accept: "text/event-stream"
			},
			body: JSON.stringify(body),
			signal
		});
		if (!res.ok) {
			const errText = await res.text().catch(() => "");
			if (res.status === 404 || /model|not found|invalid/i.test(errText)) {
				if (model === "grok-4.5" || model === "grok-4-5") return callXaiChatStream({
					...req,
					model: "grok-4.3"
				}, handlers);
				if (model === "grok-4.3") return callXaiChatStream({
					...req,
					model: "grok-4"
				}, handlers);
				if (model === "grok-4-1-fast-non-reasoning") return callXaiChatStream({
					...req,
					model: "grok-3-mini-fast"
				}, handlers);
			}
			handlers.onStatus?.("fallback");
			const full = await callXaiChat({
				...req,
				model,
				signal
			});
			if (full.ok && full.content) handlers.onDelta?.(full.content);
			return full;
		}
		if (!res.body) {
			handlers.onStatus?.("fallback");
			const full = await callXaiChat({
				...req,
				model,
				signal
			});
			if (full.ok && full.content) handlers.onDelta?.(full.content);
			return full;
		}
		handlers.onStatus?.("streaming");
		const reader = res.body.getReader();
		const decoder = new TextDecoder();
		let buffer = "";
		let content = "";
		let usedModel = model;
		let streamUsage;
		const rateLimit = parseRateLimitHeaders(res.headers);
		while (true) {
			if (signal?.aborted) {
				try {
					await reader.cancel();
				} catch {}
				return {
					ok: false,
					aborted: true,
					error: "Stopped",
					content,
					model: usedModel,
					usage: streamUsage,
					rateLimit
				};
			}
			const { done, value } = await reader.read();
			if (done) break;
			buffer += decoder.decode(value, { stream: true });
			const lines = buffer.split("\n");
			buffer = lines.pop() || "";
			for (const raw of lines) {
				const line = raw.trim();
				if (!line || line.startsWith(":")) continue;
				if (!line.startsWith("data:")) continue;
				const data = line.slice(5).trim();
				if (data === "[DONE]") continue;
				try {
					const json = JSON.parse(data);
					if (json.model) usedModel = json.model;
					if (json.usage) streamUsage = json.usage;
					const piece = json.choices?.[0]?.delta?.content || json.choices?.[0]?.message?.content || "";
					if (piece) {
						content += piece;
						handlers.onDelta?.(piece);
					}
				} catch {}
			}
		}
		if (!content.trim()) return {
			ok: false,
			error: "Empty stream from Grok",
			model: usedModel,
			rateLimit
		};
		handlers.onStatus?.("done");
		return {
			ok: true,
			content,
			model: usedModel,
			usage: streamUsage,
			rateLimit
		};
	} catch (e) {
		if (signal?.aborted || e instanceof Error && e.name === "AbortError") return {
			ok: false,
			aborted: true,
			error: "Stopped"
		};
		handlers.onStatus?.("fallback");
		try {
			const full = await callXaiChat({
				...req,
				model,
				signal
			});
			if (full.ok && full.content) handlers.onDelta?.(full.content);
			return full;
		} catch (e2) {
			if (signal?.aborted || e2 instanceof Error && e2.name === "AbortError") return {
				ok: false,
				aborted: true,
				error: "Stopped"
			};
			return {
				ok: false,
				error: e instanceof Error ? e.message : "Network error calling xAI"
			};
		}
	}
}
/** Parse HOST_CMD commands the model emits for desktop execution (own line or inline). */
function extractHostCommands(text) {
	const cmds = [];
	for (const line of text.split("\n")) {
		const m = line.match(/^\s*HOST_CMD:\s*(.+?)\s*$/i);
		if (m?.[1]) cmds.push(m[1].trim());
	}
	const inline = [...text.matchAll(/(?:^|[\s.])HOST_CMD:\s*(.+?)(?=\n|$)/gi)];
	for (const m of inline) {
		const cmd = (m[1] || "").trim();
		if (cmd && !cmds.includes(cmd)) cmds.push(cmd);
	}
	const fenced = [...text.matchAll(/```(?:host|bash|sh)\s*\n([\s\S]*?)```/gi)];
	for (const m of fenced) for (const line of (m[1] || "").split("\n")) {
		const cmd = line.trim();
		if (cmd && !cmd.startsWith("#") && !cmds.includes(cmd)) cmds.push(cmd);
	}
	return cmds.filter(Boolean);
}
/** Remove HOST_CMD markers from text shown to the user. */
function stripHostCommands(text) {
	let out = text;
	out = out.split("\n").filter((line) => !/^\s*HOST_CMD:\s*/i.test(line)).join("\n");
	out = out.replace(/\s*HOST_CMD:\s*.+$/gim, "");
	out = out.replace(/```(?:host|bash|sh)\s*\n[\s\S]*?```/gi, "");
	return out.replace(/\n{3,}/g, "\n\n").trim();
}
/**
* If the user clearly asks about local files/folders and the model forgot HOST_CMD,
* invent a safe listing command.
*/
function inferHostCommandsFromUser(prompt) {
	const p = prompt.toLowerCase();
	const wantsList = /\b(list|show|what('|’)?s|whats|what do i have|contents?|files?|inside|in my)\b/.test(p) || /\b(check|look at|open)\b/.test(p);
	if (!wantsList && !/\b(download|downloads|desktop|documents|home|folder|directory)\b/.test(p)) return [];
	if (/\bdownloads?\b/.test(p)) return ["ls -la \"${HOME}/Downloads\" 2>/dev/null || ls -la ~/Downloads 2>/dev/null || ls -la \"$HOME/Descargas\" 2>/dev/null || echo \"Downloads folder not found\""];
	if (/\bdocuments?\b/.test(p)) return ["ls -la \"${HOME}/Documents\" 2>/dev/null || ls -la ~/Documents 2>/dev/null || echo \"Documents folder not found\""];
	if (/\bdesktop\b/.test(p)) return ["ls -la \"${HOME}/Desktop\" 2>/dev/null || ls -la ~/Desktop 2>/dev/null || echo \"Desktop folder not found\""];
	if (/\bhome\b/.test(p) && wantsList) return ["ls -la \"$HOME\" | head -80"];
	return [];
}
async function probeXaiKey(apiKey) {
	const key = apiKey.trim();
	if (!key) return {
		ok: false,
		detail: "API key is empty"
	};
	try {
		const res = await fetch(`${XAI_BASE}/models`, { headers: { authorization: `Bearer ${key}` } });
		if (res.ok) return {
			ok: true,
			detail: "Connected to xAI · models reachable"
		};
		const text = await res.text();
		return {
			ok: false,
			detail: `xAI ${res.status}: ${text.slice(0, 160)}`
		};
	} catch (e) {
		return {
			ok: false,
			detail: e instanceof Error ? e.message : "probe failed"
		};
	}
}
async function probeXaiBearer(bearer) {
	return probeXaiKey(bearer);
}
/** Live Grok / xAI image generation (falls through models if one id is unavailable). */
async function callXaiImagine(req) {
	const auth = resolveBearer({
		accessToken: req.accessToken,
		apiKey: req.apiKey,
		messages: []
	});
	if (!auth) return {
		ok: false,
		error: "Not connected — Grok OAuth or API key required for live Imagine"
	};
	const prompt = req.prompt.trim();
	if (!prompt) return {
		ok: false,
		error: "empty prompt"
	};
	const models = [
		req.model,
		"grok-2-image",
		"grok-2-image-1212",
		"grok-imagine-image"
	].filter(Boolean);
	let lastErr = "image generation failed";
	for (const model of models) try {
		const res = await fetch(`${XAI_BASE}/images/generations`, {
			method: "POST",
			headers: {
				"content-type": "application/json",
				authorization: `Bearer ${auth.bearer}`
			},
			body: JSON.stringify({
				model,
				prompt,
				n: 1,
				response_format: "b64_json"
			})
		});
		const data = await res.json().catch(() => ({}));
		if (!res.ok) {
			lastErr = typeof data.error === "string" ? data.error : data.error?.message || `xAI image ${res.status} (${model})`;
			continue;
		}
		const row = data.data?.[0];
		const b64 = row?.b64_json || row?.b64 || row?.image || "";
		if (b64) return {
			ok: true,
			imageDataUrl: b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`,
			model: data.model || model,
			source: "xai"
		};
		if (row?.url) return {
			ok: true,
			imageDataUrl: row.url,
			model: data.model || model,
			source: "xai"
		};
		lastErr = "empty image response";
	} catch (e) {
		lastErr = e instanceof Error ? e.message : "network error";
	}
	return {
		ok: false,
		error: lastErr
	};
}
//#endregion
export { XAI_BASE, callXaiChat, callXaiChatStream, callXaiImagine, extractHostCommands, inferHostCommandsFromUser, probeXaiBearer, stripHostCommands };
