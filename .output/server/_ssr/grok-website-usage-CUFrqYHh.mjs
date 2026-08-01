import { r as __exportAll } from "../_runtime.mjs";
import { t as __exportAll$1 } from "./rolldown-runtime-D7D4PA-g.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/grok-website-usage-CUFrqYHh.js
var grok_website_usage_CUFrqYHh_exports = /* @__PURE__ */ __exportAll({
	n: () => formatUsdFromCents,
	r: () => grok_website_usage_exports,
	t: () => formatResetAt
});
var grok_website_usage_exports = /* @__PURE__ */ __exportAll$1({
	fetchGrokWebsiteUsage: () => fetchGrokWebsiteUsage,
	fetchGrokWebsiteUsageDirect: () => fetchGrokWebsiteUsageDirect,
	formatResetAt: () => formatResetAt,
	formatUsdFromCents: () => formatUsdFromCents
});
var PRODUCT_LABELS = {
	0: {
		id: "other",
		label: "Other"
	},
	1: {
		id: "api",
		label: "API"
	},
	2: {
		id: "build",
		label: "Grok Build"
	},
	3: {
		id: "plugins",
		label: "Plugins"
	},
	4: {
		id: "chat",
		label: "Chat"
	},
	5: {
		id: "imagine",
		label: "Imagine"
	},
	6: {
		id: "voice",
		label: "Voice"
	},
	7: {
		id: "app_builder",
		label: "App Builder"
	}
};
var GROK_CREDITS_URL = "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
var SUBSCRIPTIONS_URL = "https://grok.com/rest/subscriptions";
function grpcWebFrame(payload) {
	const out = new Uint8Array(5 + payload.length);
	out[0] = 0;
	const len = payload.length;
	out[1] = len >>> 24 & 255;
	out[2] = len >>> 16 & 255;
	out[3] = len >>> 8 & 255;
	out[4] = len & 255;
	out.set(payload, 5);
	return out;
}
/** GetGrokCreditsConfigRequest { exclude_legacy_monthly_usage = true } */
function encodeCreditsRequest() {
	return new Uint8Array([8, 1]);
}
function readVarint(buf, offset) {
	let result = 0;
	let shift = 0;
	let pos = offset;
	while (pos < buf.length) {
		const b = buf[pos++];
		result |= (b & 127) << shift;
		if ((b & 128) === 0) break;
		shift += 7;
		if (shift > 35) break;
	}
	return {
		value: result >>> 0,
		next: pos
	};
}
function readVarintBig(buf, offset) {
	let result = 0n;
	let shift = 0n;
	let pos = offset;
	while (pos < buf.length) {
		const b = BigInt(buf[pos++]);
		result |= (b & 127n) << shift;
		if ((b & 128n) === 0n) break;
		shift += 7n;
	}
	return {
		value: result,
		next: pos
	};
}
/** Minimal protobuf wire decoder → nested plain objects keyed by field number. */
function decodeFields(buf) {
	const out = {};
	let i = 0;
	while (i < buf.length) {
		const tag = readVarint(buf, i);
		i = tag.next;
		const field = tag.value >>> 3;
		const wire = tag.value & 7;
		if (field === 0) break;
		if (wire === 0) {
			const v = readVarintBig(buf, i);
			i = v.next;
			(out[field] ||= []).push(v.value);
		} else if (wire === 1) {
			if (i + 8 > buf.length) break;
			const slice = buf.slice(i, i + 8);
			i += 8;
			(out[field] ||= []).push(slice);
		} else if (wire === 2) {
			const len = readVarint(buf, i);
			i = len.next;
			const slice = buf.slice(i, i + len.value);
			i += len.value;
			(out[field] ||= []).push(slice);
		} else if (wire === 5) {
			if (i + 4 > buf.length) break;
			const slice = buf.slice(i, i + 4);
			i += 4;
			(out[field] ||= []).push(slice);
		} else break;
	}
	return out;
}
function asBytes(v) {
	return v instanceof Uint8Array ? v : null;
}
function asBig(v) {
	return typeof v === "bigint" ? v : null;
}
function decodeDouble(bytes) {
	if (bytes.length < 8) return 0;
	return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getFloat64(0, true);
}
function decodeTimestamp(bytes) {
	const f = decodeFields(bytes);
	const sec = asBig(f[1]?.[0]);
	const nanos = asBig(f[2]?.[0]);
	if (sec == null) return null;
	const ms = Number(sec) * 1e3 + (nanos != null ? Number(nanos) / 1e6 : 0);
	return Number.isFinite(ms) ? ms : null;
}
function decodeCent(bytes) {
	const val = asBig(decodeFields(bytes)[1]?.[0]);
	return val != null ? Number(val) : 0;
}
function parseGrpcWebResponse(buf) {
	let i = 0;
	const messages = [];
	let status = 0;
	let message = "";
	while (i + 5 <= buf.length) {
		const flag = buf[i];
		const len = (buf[i + 1] << 24 | buf[i + 2] << 16 | buf[i + 3] << 8 | buf[i + 4]) >>> 0;
		i += 5;
		if (i + len > buf.length) break;
		const chunk = buf.slice(i, i + len);
		i += len;
		if (flag === 0) messages.push(chunk);
		else if (flag === 128 || flag === 128) {
			const text = new TextDecoder().decode(chunk);
			const statusMatch = /grpc-status:\s*(\d+)/i.exec(text);
			const msgMatch = /grpc-message:\s*([^\r\n]+)/i.exec(text);
			if (statusMatch) status = Number(statusMatch[1]);
			if (msgMatch) message = decodeURIComponent(msgMatch[1].replace(/\+/g, " "));
		}
	}
	return {
		status,
		message,
		messages
	};
}
function parseCreditsConfig(msg) {
	const configBytes = asBytes(decodeFields(msg)[1]?.[0]);
	if (!configBytes) return {
		creditUsagePercent: 0,
		periodType: "unknown",
		periodStart: null,
		periodEnd: null,
		productUsage: [],
		prepaidBalanceCents: 0,
		onDemandCapCents: 0,
		onDemandUsedCents: 0
	};
	const c = decodeFields(configBytes);
	let periodType = "unknown";
	let periodStart = null;
	let periodEnd = null;
	const periodBytes = asBytes(c[1]?.[0]);
	if (periodBytes) {
		const p = decodeFields(periodBytes);
		const t = asBig(p[1]?.[0]);
		if (t === 2n) periodType = "weekly";
		else if (t === 1n) periodType = "monthly";
		const s = asBytes(p[2]?.[0]);
		const e = asBytes(p[3]?.[0]);
		if (s) periodStart = decodeTimestamp(s);
		if (e) periodEnd = decodeTimestamp(e);
	}
	let creditUsagePercent = 0;
	const pctBytes = asBytes(c[2]?.[0]);
	if (pctBytes && pctBytes.length === 8) creditUsagePercent = decodeDouble(pctBytes);
	else {
		const f5 = asBytes(c[2]?.[0]);
		if (f5 && f5.length === 4) creditUsagePercent = new DataView(f5.buffer, f5.byteOffset, 4).getFloat32(0, true);
	}
	const onDemandCapCents = asBytes(c[3]?.[0]) ? decodeCent(asBytes(c[3][0])) : 0;
	const onDemandUsedCents = asBytes(c[4]?.[0]) ? decodeCent(asBytes(c[4][0])) : 0;
	const productUsage = [];
	for (const raw of c[5] || []) {
		const b = asBytes(raw);
		if (!b) continue;
		const pu = decodeFields(b);
		const meta = PRODUCT_LABELS[Number(asBig(pu[1]?.[0]) ?? 0)] || PRODUCT_LABELS[0];
		let usagePercent = 0;
		const up = asBytes(pu[2]?.[0]);
		if (up && up.length === 8) usagePercent = decodeDouble(up);
		else if (up && up.length === 4) usagePercent = new DataView(up.buffer, up.byteOffset, 4).getFloat32(0, true);
		if (usagePercent > 0) productUsage.push({
			product: meta.id,
			label: meta.label,
			usagePercent
		});
	}
	productUsage.sort((a, b) => b.usagePercent - a.usagePercent);
	const prepaidBalanceCents = asBytes(c[8]?.[0]) ? decodeCent(asBytes(c[8][0])) : 0;
	const bStart = asBytes(c[10]?.[0]);
	const bEnd = asBytes(c[11]?.[0]);
	if (bStart) periodStart = decodeTimestamp(bStart) ?? periodStart;
	if (bEnd) periodEnd = decodeTimestamp(bEnd) ?? periodEnd;
	return {
		creditUsagePercent: Math.max(0, creditUsagePercent),
		periodType,
		periodStart,
		periodEnd,
		productUsage,
		prepaidBalanceCents,
		onDemandCapCents,
		onDemandUsedCents
	};
}
function planFromSubscriptions(json) {
	try {
		const subs = json?.subscriptions || [];
		const active = subs.find((s) => {
			const st = String(s.status || s.state || "").toLowerCase();
			return !st || st.includes("active") || st.includes("trial");
		}) || subs[0];
		if (!active) return {
			planLabel: "SuperGrok",
			planId: "super"
		};
		const tier = String(active.tier || active.plan || active.product || active.name || active.subscriptionTier || "").toLowerCase();
		if (tier.includes("heavy") || tier.includes("pro")) return {
			planLabel: "SuperGrok Heavy",
			planId: "heavy"
		};
		if (tier.includes("plus")) return {
			planLabel: "SuperGrok Plus",
			planId: "plus"
		};
		if (tier.includes("lite")) return {
			planLabel: "SuperGrok Lite",
			planId: "lite"
		};
		if (tier.includes("free")) return {
			planLabel: "Free",
			planId: "free"
		};
		return {
			planLabel: "SuperGrok",
			planId: "super"
		};
	} catch {
		return {
			planLabel: "SuperGrok",
			planId: "super"
		};
	}
}
function buildCookieHeader(sso) {
	const t = sso.trim();
	if (!t) return "";
	if (t.toLowerCase().includes("sso=")) return t;
	return `sso=${t}`;
}
/**
* Fetch weekly SuperGrok usage from the same endpoint the Grok website uses.
* In the browser, routes through our server proxy to avoid CORS.
*/
async function fetchGrokWebsiteUsage(auth) {
	const cookie = buildCookieHeader(auth.ssoCookie || "");
	const bearer = (auth.bearer || "").trim();
	if (!cookie && !bearer) return {
		ok: false,
		error: "Connect your Grok website session (SSO) to load weekly SuperGrok usage. Device OAuth alone does not expose website quotas.",
		planLabel: "—",
		planId: "free",
		creditUsagePercent: 0,
		periodType: "unknown",
		periodStart: null,
		periodEnd: null,
		productUsage: [],
		prepaidBalanceCents: 0,
		onDemandCapCents: 0,
		onDemandUsedCents: 0
	};
	if (typeof window !== "undefined") {
		const desktop = window.grokhubDesktop?.grok;
		if (desktop?.websiteUsage) try {
			return await desktop.websiteUsage({
				ssoCookie: cookie,
				bearer
			});
		} catch {}
		try {
			const res = await fetch("/api/grok", {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({
					action: "websiteUsage",
					ssoCookie: cookie,
					bearer
				})
			});
			if (res.ok) return await res.json();
		} catch {}
	}
	return fetchGrokWebsiteUsageDirect(auth);
}
/** Direct call to grok.com (server / Electron main). */
async function fetchGrokWebsiteUsageDirect(auth) {
	const cookie = buildCookieHeader(auth.ssoCookie || "");
	const bearer = (auth.bearer || "").trim();
	if (!cookie && !bearer) return {
		ok: false,
		error: "No Grok website session",
		planLabel: "—",
		planId: "free",
		creditUsagePercent: 0,
		periodType: "unknown",
		periodStart: null,
		periodEnd: null,
		productUsage: [],
		prepaidBalanceCents: 0,
		onDemandCapCents: 0,
		onDemandUsedCents: 0
	};
	const headers = {
		"content-type": "application/grpc-web+proto",
		"x-grpc-web": "1",
		accept: "application/grpc-web+proto",
		origin: "https://grok.com",
		referer: "https://grok.com/settings",
		"user-agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 GrokHub/0.2.14"
	};
	if (cookie) headers.cookie = cookie;
	if (bearer) headers.authorization = `Bearer ${bearer}`;
	try {
		const payload = grpcWebFrame(encodeCreditsRequest());
		const res = await fetch(GROK_CREDITS_URL, {
			method: "POST",
			headers,
			body: payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength)
		});
		const headerStatus = res.headers.get("grpc-status");
		const headerMsg = res.headers.get("grpc-message");
		const parsed = parseGrpcWebResponse(new Uint8Array(await res.arrayBuffer()));
		const status = parsed.status || (headerStatus != null ? Number(headerStatus) : 0);
		const message = parsed.message || (headerMsg ? decodeURIComponent(headerMsg.replace(/\+/g, " ")) : "") || (!res.ok ? `HTTP ${res.status}` : "");
		if (status !== 0 || !parsed.messages[0]) return {
			ok: false,
			error: message || (status === 16 ? "Website session expired — re-link Grok SSO in Settings." : `Grok usage error (grpc ${status})`),
			planLabel: "—",
			planId: "free",
			creditUsagePercent: 0,
			periodType: "unknown",
			periodStart: null,
			periodEnd: null,
			productUsage: [],
			prepaidBalanceCents: 0,
			onDemandCapCents: 0,
			onDemandUsedCents: 0
		};
		const usage = parseCreditsConfig(parsed.messages[0]);
		let planLabel = "SuperGrok";
		let planId = "super";
		try {
			const subHeaders = {
				accept: "application/json",
				origin: "https://grok.com",
				referer: "https://grok.com/settings"
			};
			if (cookie) subHeaders.cookie = cookie;
			if (bearer) subHeaders.authorization = `Bearer ${bearer}`;
			const subRes = await fetch(SUBSCRIPTIONS_URL, { headers: subHeaders });
			if (subRes.ok) {
				const mapped = planFromSubscriptions(await subRes.json());
				planLabel = mapped.planLabel;
				planId = mapped.planId;
			}
		} catch {}
		if (usage.periodType === "weekly" && planId === "heavy") planLabel = "SuperGrok Heavy";
		else if (usage.periodType === "weekly" && planId === "pro") {
			planLabel = "SuperGrok Heavy";
			planId = "heavy";
		}
		return {
			ok: true,
			planLabel,
			planId,
			...usage
		};
	} catch (e) {
		return {
			ok: false,
			error: e instanceof Error ? e.message : "Failed to reach grok.com usage API",
			planLabel: "—",
			planId: "free",
			creditUsagePercent: 0,
			periodType: "unknown",
			periodStart: null,
			periodEnd: null,
			productUsage: [],
			prepaidBalanceCents: 0,
			onDemandCapCents: 0,
			onDemandUsedCents: 0
		};
	}
}
function formatResetAt(ts) {
	if (!ts) return "—";
	try {
		return new Date(ts).toLocaleString(void 0, {
			month: "long",
			day: "numeric",
			year: "numeric",
			hour: "numeric",
			minute: "2-digit"
		});
	} catch {
		return "—";
	}
}
function formatUsdFromCents(cents) {
	return ((cents || 0) / 100).toLocaleString(void 0, {
		style: "currency",
		currency: "USD"
	});
}
//#endregion
export { formatUsdFromCents as n, grok_website_usage_CUFrqYHh_exports as r, formatResetAt as t };
