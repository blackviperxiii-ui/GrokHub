globalThis.__nitro_main__ = import.meta.url;
import { a as toEventHandler, c as serve, i as defineLazyEventHandler, n as HTTPError, r as defineHandler, s as NodeResponse, t as H3Core } from "./_libs/h3+rou3+srvx.mjs";
import { i as withoutTrailingSlash, n as joinURL, r as withLeadingSlash, t as decodePath } from "./_libs/ufo.mjs";
import { promises } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
//#region #nitro-vite-setup
function lazyService(loader) {
	let promise, mod;
	return { fetch(req) {
		if (mod) return mod.fetch(req);
		if (!promise) promise = loader().then((_mod) => mod = _mod.default || _mod);
		return promise.then((mod) => mod.fetch(req));
	} };
}
var services = { ["ssr"]: lazyService(() => import("./_ssr/ssr.mjs")) };
globalThis.__nitro_vite_envs__ = services;
//#endregion
//#region node_modules/nitro/dist/runtime/internal/route-rules.mjs
var headers = ((m) => function headersRouteRule(event) {
	for (const [key, value] of Object.entries(m.options || {})) event.res.headers.set(key, value);
});
//#endregion
//#region #nitro/virtual/public-assets-data
var public_assets_data_default = {
	"/assets/AgentsView-Dk5U0aJl.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"cc6-fRI5YUwCJ9wH8SO5GTeNsikCRH4\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 3270,
		"path": "../public/assets/AgentsView-Dk5U0aJl.js"
	},
	"/assets/AutomationsView-S_DWDWiZ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"103f-g276Bhp3G1HvEu3OfPpWiD4341A\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 4159,
		"path": "../public/assets/AutomationsView-S_DWDWiZ.js"
	},
	"/assets/ChatView-BLS1lpCq.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2a515-dqu/hCCQSSlsvtoaaTKchay7/Xc\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 173333,
		"path": "../public/assets/ChatView-BLS1lpCq.js"
	},
	"/assets/CommandView-B2jFiDM4.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1b4f-0M9oDL+dxkVnJCFe4qlUqQa+588\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 6991,
		"path": "../public/assets/CommandView-B2jFiDM4.js"
	},
	"/assets/ConnectorsView-CJ_DqO1H.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d69-QC8FNvdnzTci/q/ufTVjyOZD9pQ\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 3433,
		"path": "../public/assets/ConnectorsView-CJ_DqO1H.js"
	},
	"/assets/DesktopHostView-yy1mzH-3.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2ae8-2tRySukdC/lHQ10PktyMk99jKTI\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 10984,
		"path": "../public/assets/DesktopHostView-yy1mzH-3.js"
	},
	"/assets/GrokLogo-D_dsCbfo.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"497-DuGx9ic7WwY1oBLTQJekAcB7bcM\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 1175,
		"path": "../public/assets/GrokLogo-D_dsCbfo.js"
	},
	"/assets/HistoryView-BSjQQPY-.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"e3a-zMBaOqpJjBhKDS0qdSjduq1e+0Q\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 3642,
		"path": "../public/assets/HistoryView-BSjQQPY-.js"
	},
	"/assets/HostGatewayBanner-YfKwYIC1.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"18b9-jJEnpGWKSv/z/LH4Rsjw5B8kksU\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 6329,
		"path": "../public/assets/HostGatewayBanner-YfKwYIC1.js"
	},
	"/assets/ImagineView-CP1mYF5m.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"12e0-fT8E6zHhNVKjCkMwuRfLw6uH7EA\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 4832,
		"path": "../public/assets/ImagineView-CP1mYF5m.js"
	},
	"/assets/ProfileAvatar-DtjRjG9c.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"32e-z9NdnWEp4/2asq3UF58tiyhSeS0\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 814,
		"path": "../public/assets/ProfileAvatar-DtjRjG9c.js"
	},
	"/assets/RelativeTime-tF303D4T.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"165-xSjpGdAAcbO3mFS7S8z775hH7vs\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 357,
		"path": "../public/assets/RelativeTime-tF303D4T.js"
	},
	"/assets/SettingsView-CCgNt4Ww.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"3c51-Ua+1ymN3qyWL0VYmryKWB5rzLf8\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 15441,
		"path": "../public/assets/SettingsView-CCgNt4Ww.js"
	},
	"/assets/SkillsView-CKtFbHZo.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1318-0BOoNVMT8xyhq5zSvZtlOF5IH3A\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 4888,
		"path": "../public/assets/SkillsView-CKtFbHZo.js"
	},
	"/assets/UsageMeter-C6BeI3Pl.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1d6e-UYvu8KLDAgNdu4tNTaGF9zJCdiE\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 7534,
		"path": "../public/assets/UsageMeter-C6BeI3Pl.js"
	},
	"/assets/automation-schedule-CxNhus6_.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"3c1-YP+1AoGh2pMNInxr9Dt+s3wLOgs\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 961,
		"path": "../public/assets/automation-schedule-CxNhus6_.js"
	},
	"/assets/button-PTui52Sr.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"51c-N3/mqHNn45iABnC8wcg2FW+cfug\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 1308,
		"path": "../public/assets/button-PTui52Sr.js"
	},
	"/assets/cable-CFvVYaMP.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1a0-tykz95e/QKzID6NSrXpJV83gcgY\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 416,
		"path": "../public/assets/cable-CFvVYaMP.js"
	},
	"/assets/card-CuyxkQXR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"ea62-XE6AiKXiJzcjBNLg/dJpP8ZNN28\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 60002,
		"path": "../public/assets/card-CuyxkQXR.js"
	},
	"/assets/dist-CLNNfRwf.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"6e94-GgJ+wN0kekbnNp3bqPoZahoBo2Q\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 28308,
		"path": "../public/assets/dist-CLNNfRwf.js"
	},
	"/assets/folder-input-DYk-DmWD.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"12e-nslOs/1hYO+ZVjnQmpD04PZQavg\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 302,
		"path": "../public/assets/folder-input-DYk-DmWD.js"
	},
	"/assets/gauge-Bh27Zv6C.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"a4-YaZx9WdxoKJLZvzSUNhwUaHrn/I\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 164,
		"path": "../public/assets/gauge-Bh27Zv6C.js"
	},
	"/assets/grok-DaaXHfHa.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"5f6-3yZyFmC8ExoU9QlMjc0zXyjkpi4\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 1526,
		"path": "../public/assets/grok-DaaXHfHa.js"
	},
	"/assets/grok-client-DHaxRp7l.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dce-Si0Oe4fW/D0IXkzuDisdCguf/5M\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 3534,
		"path": "../public/assets/grok-client-DHaxRp7l.js"
	},
	"/assets/grok-website-usage-Dqpi-Tiv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1caa-IHUMAs9KQb+sXyPIgKQjGqxYp7o\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 7338,
		"path": "../public/assets/grok-website-usage-Dqpi-Tiv.js"
	},
	"/assets/hard-drive-CNosmw-g.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"17b-dPZCyK1P4RUX4w2r6qhSBVtNR40\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 379,
		"path": "../public/assets/hard-drive-CNosmw-g.js"
	},
	"/assets/host-client-BqP38civ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"925-Vsdw6E6cdholAWaa/woxQMk+yZQ\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 2341,
		"path": "../public/assets/host-client-BqP38civ.js"
	},
	"/assets/host-safety-F25AUCFv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"64a-SpuiaxmBInIV0k2ISOAF+XXdA1A\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 1610,
		"path": "../public/assets/host-safety-F25AUCFv.js"
	},
	"/assets/image-CCc5CW0s.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-xE9pNrC5BjoxlSYYbM+C0d7mclQ\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 257,
		"path": "../public/assets/image-CCc5CW0s.js"
	},
	"/assets/index-BXjOc1u_.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"4a99a-QS0nCB1ypPXlQPtnfth5Jtgn08c\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 305562,
		"path": "../public/assets/index-BXjOc1u_.js"
	},
	"/assets/input-stfai6_O.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-bdJHDEhj588XZ8GpznevfxMMht8\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 581,
		"path": "../public/assets/input-stfai6_O.js"
	},
	"/assets/jsx-runtime-KJkY8l8U.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2157-uh2PnvJKYWZAlieFni6eRY8YAVs\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 8535,
		"path": "../public/assets/jsx-runtime-KJkY8l8U.js"
	},
	"/assets/loader-circle-BYDVgGz7.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"84-0K6AC4nj97If69arginop9u7PJc\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 132,
		"path": "../public/assets/loader-circle-BYDVgGz7.js"
	},
	"/assets/login-CN5S9mvy.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"69b-yKS0TylKUefPSFTLsOxqQ/fmIyE\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 1691,
		"path": "../public/assets/login-CN5S9mvy.js"
	},
	"/assets/message-square-plus-B5v9uxT8.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f4-q787TbexARqXlWAeUaRV48u5B8s\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 244,
		"path": "../public/assets/message-square-plus-B5v9uxT8.js"
	},
	"/assets/openclaw-import-CRJUnD2p.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f7a-2XiG/hFckln0bhpCVuqz3wahUq8\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 3962,
		"path": "../public/assets/openclaw-import-CRJUnD2p.js"
	},
	"/assets/play-lu9hwWCv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"7a-eEMyUOYrJ2b5+c0D/ApNp/Jz/kc\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 122,
		"path": "../public/assets/play-lu9hwWCv.js"
	},
	"/assets/plus-COKy4pTB.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8d-8MUfASSgm3b5wilARYPwa3IIpLc\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 141,
		"path": "../public/assets/plus-COKy4pTB.js"
	},
	"/assets/react-dom-BsrxZfdi.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dda-/KkbQdWpmEcddLNsaiMx/D5PdtI\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 3546,
		"path": "../public/assets/react-dom-BsrxZfdi.js"
	},
	"/assets/refresh-cw-BVfm9tmU.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"135-ZI4zljSACllDyW6XqA0Pn/QkG/0\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 309,
		"path": "../public/assets/refresh-cw-BVfm9tmU.js"
	},
	"/assets/routes-BuReIHa9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"5bed-5OCeF2LncmxkcXWe0EK3fc2+Sk4\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 23533,
		"path": "../public/assets/routes-BuReIHa9.js"
	},
	"/assets/secrets-client-BSOFMExG.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"257-lWqWk5xSkICzrvrvNCTDhqN731k\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 599,
		"path": "../public/assets/secrets-client-BSOFMExG.js"
	},
	"/assets/sparkles-CpiQxR91.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1f0-9tPyML9O2haS5wZdmPz0tmoyAR8\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 496,
		"path": "../public/assets/sparkles-CpiQxR91.js"
	},
	"/assets/square-RXMzgFIe.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"87-qz/DXuf/LZuPKgPr+VUEKWn+Qp4\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 135,
		"path": "../public/assets/square-RXMzgFIe.js"
	},
	"/assets/styles-C09y9G3u.css": {
		"type": "text/css; charset=utf-8",
		"etag": "\"c154-pmgFDq3ZrSrCoGXWUQ8re0GEgLg\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 49492,
		"path": "../public/assets/styles-C09y9G3u.css"
	},
	"/assets/terminal-CnquOmvK.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"b3-2cRMhJxrd8Caq/7AdWXqpXNLLpI\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 179,
		"path": "../public/assets/terminal-CnquOmvK.js"
	},
	"/assets/textarea-CzzBL_GL.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-21uDSBgHx4/9QT/pvTZA0Ucssdk\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 581,
		"path": "../public/assets/textarea-CzzBL_GL.js"
	},
	"/assets/timer-reset-CuJBFmH9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-LcjBdNXWhduPmpiooMkztaplofI\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 257,
		"path": "../public/assets/timer-reset-CuJBFmH9.js"
	},
	"/assets/trash-2-7EK6ZqAR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2e7-TlX9lSZCnyLjo08cpExDC/nY4XU\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 743,
		"path": "../public/assets/trash-2-7EK6ZqAR.js"
	},
	"/assets/use-current-user-BDqHjrBD.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"79be-4sntSCUxL9dv+F+8iqvBAPDoAhU\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 31166,
		"path": "../public/assets/use-current-user-BDqHjrBD.js"
	},
	"/assets/users-wT5ZySaU.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"126-odqO6McFzL3HhwuR3o3eEL0Brdc\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 294,
		"path": "../public/assets/users-wT5ZySaU.js"
	},
	"/assets/zap-DgjUbPpr.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"fa-UQKTFuA0G7joSd2WbzcznoBDykE\"",
		"mtime": "2026-08-01T22:47:20.965Z",
		"size": 250,
		"path": "../public/assets/zap-DgjUbPpr.js"
	}
};
//#endregion
//#region #nitro/virtual/public-assets-node
function readAsset(id) {
	const serverDir = dirname(fileURLToPath(globalThis.__nitro_main__));
	return promises.readFile(resolve(serverDir, public_assets_data_default[id].path));
}
//#endregion
//#region #nitro/virtual/public-assets
var publicAssetBases = {};
function isPublicAssetURL(id = "") {
	if (public_assets_data_default[id]) return true;
	for (const base in publicAssetBases) if (id.startsWith(base)) return true;
	return false;
}
function getAsset(id) {
	return public_assets_data_default[id];
}
//#endregion
//#region node_modules/nitro/dist/runtime/internal/static.mjs
var METHODS = /* @__PURE__ */ new Set(["HEAD", "GET"]);
var EncodingMap = {
	gzip: ".gz",
	br: ".br",
	zstd: ".zst"
};
var static_default = defineHandler((event) => {
	if (event.req.method && !METHODS.has(event.req.method)) return;
	let id = decodePath(withLeadingSlash(withoutTrailingSlash(event.url.pathname)));
	let asset;
	const encodings = [...(event.req.headers.get("accept-encoding") || "").split(",").map((e) => EncodingMap[e.trim()]).filter(Boolean).sort(), ""];
	for (const encoding of encodings) for (const _id of [id + encoding, joinURL(id, "index.html" + encoding)]) {
		const _asset = getAsset(_id);
		if (_asset) {
			asset = _asset;
			id = _id;
			break;
		}
	}
	if (!asset) {
		if (isPublicAssetURL(id)) {
			event.res.headers.delete("Cache-Control");
			throw new HTTPError({ status: 404 });
		}
		return;
	}
	if (encodings.length > 1) event.res.headers.append("Vary", "Accept-Encoding");
	if (event.req.headers.get("if-none-match") === asset.etag) {
		event.res.status = 304;
		event.res.statusText = "Not Modified";
		return "";
	}
	const ifModifiedSinceH = event.req.headers.get("if-modified-since");
	const mtimeDate = new Date(asset.mtime);
	if (ifModifiedSinceH && asset.mtime && new Date(ifModifiedSinceH) >= mtimeDate) {
		event.res.status = 304;
		event.res.statusText = "Not Modified";
		return "";
	}
	if (asset.type) event.res.headers.set("Content-Type", asset.type);
	if (asset.etag && !event.res.headers.has("ETag")) event.res.headers.set("ETag", asset.etag);
	if (asset.mtime && !event.res.headers.has("Last-Modified")) event.res.headers.set("Last-Modified", mtimeDate.toUTCString());
	if (asset.encoding && !event.res.headers.has("Content-Encoding")) event.res.headers.set("Content-Encoding", asset.encoding);
	if (asset.size > 0 && !event.res.headers.has("Content-Length")) event.res.headers.set("Content-Length", asset.size.toString());
	return readAsset(id);
});
//#endregion
//#region #nitro/virtual/routing
var findRouteRules = /* @__PURE__ */ (() => {
	const $0 = [{
		name: "headers",
		route: "/assets/**",
		handler: headers,
		options: { "cache-control": "public, max-age=31536000, immutable" }
	}];
	return (m, p) => {
		let r = [];
		if (p.charCodeAt(p.length - 1) === 47) p = p.slice(0, -1) || "/";
		let s = p.split("/");
		if (s.length > 1) {
			if (s[1] === "assets") r.unshift({
				data: $0,
				params: { "_": s.slice(2).join("/") }
			});
		}
		return r;
	};
})();
var _lazy_IO091Z = defineLazyEventHandler(() => import("./_chunks/ssr-renderer.mjs"));
var findRoute = /* @__PURE__ */ (() => {
	const data = {
		route: "/**",
		handler: _lazy_IO091Z
	};
	return ((_m, p) => {
		return {
			data,
			params: { "_": p.slice(1) }
		};
	});
})();
var globalMiddleware = [toEventHandler(static_default)].filter(Boolean);
//#endregion
//#region node_modules/nitro/dist/runtime/internal/error/prod.mjs
var errorHandler = (error, event) => {
	const res = defaultHandler(error, event);
	return new NodeResponse(typeof res.body === "string" ? res.body : JSON.stringify(res.body, null, 2), res);
};
function defaultHandler(error, event) {
	const unhandled = error.unhandled ?? !HTTPError.isError(error);
	const { status = 500, statusText = "" } = unhandled ? {} : error;
	if (status === 404) {
		const url = event.url || new URL(event.req.url);
		const baseURL = "/";
		if (/^\/[^/]/.test(baseURL) && !url.pathname.startsWith(baseURL)) return {
			status: 302,
			headers: new Headers({ location: `${baseURL}${url.pathname.slice(1)}${url.search}` })
		};
	}
	const headers = new Headers(unhandled ? {} : error.headers);
	headers.set("content-type", "application/json; charset=utf-8");
	return {
		status,
		statusText,
		headers,
		body: {
			error: true,
			...unhandled ? {
				status,
				unhandled: true
			} : typeof error.toJSON === "function" ? error.toJSON() : {
				status,
				statusText,
				message: error.message
			}
		}
	};
}
//#endregion
//#region #nitro/virtual/error-handler
var errorHandlers = [errorHandler];
async function error_handler_default(error, event) {
	for (const handler of errorHandlers) try {
		const response = await handler(error, event, { defaultHandler });
		if (response) return response;
	} catch (error) {
		console.error(error);
	}
}
//#endregion
//#region #nitro/virtual/app
function createNitroApp() {
	const captureError = (error, errorCtx) => {
		if (errorCtx?.event) {
			const errors = errorCtx.event.req.context?.nitro?.errors;
			if (errors) errors.push({
				error,
				context: errorCtx
			});
		}
	};
	const h3App = createH3App({ onError(error, event) {
		return error_handler_default(error, event);
	} });
	let appHandler = (req) => {
		req.context ||= {};
		req.context.nitro = req.context.nitro || { errors: [] };
		return h3App.fetch(req);
	};
	return {
		fetch: appHandler,
		h3: h3App,
		hooks: void 0,
		captureError
	};
}
function createH3App(config) {
	const h3App = new H3Core(config);
	h3App["~findRoute"] = (event) => findRoute(event.req.method, event.url.pathname);
	h3App["~middleware"].push(...globalMiddleware);
	h3App["~getMiddleware"] = (event, route) => {
		const pathname = event.url.pathname;
		const method = event.req.method;
		const middleware = [];
		const routeRules = getRouteRules(method, pathname);
		event.context.routeRules = routeRules?.routeRules;
		if (routeRules?.routeRuleMiddleware.length) middleware.push(...routeRules.routeRuleMiddleware);
		middleware.push(...h3App["~middleware"]);
		if (route?.data?.middleware?.length) middleware.push(...route.data.middleware);
		return middleware;
	};
	return h3App;
}
//#endregion
//#region node_modules/nitro/dist/runtime/internal/app.mjs
var APP_ID = "default";
function useNitroApp() {
	let instance = useNitroApp._instance;
	if (instance) return instance;
	instance = useNitroApp._instance = createNitroApp();
	globalThis.__nitro__ = globalThis.__nitro__ || {};
	globalThis.__nitro__[APP_ID] = instance;
	return instance;
}
function getRouteRules(method, pathname) {
	const m = findRouteRules(method, pathname);
	if (!m?.length) return { routeRuleMiddleware: [] };
	const routeRules = {};
	for (const layer of m) for (const rule of layer.data) {
		const currentRule = routeRules[rule.name];
		if (currentRule) {
			if (rule.options === false) {
				delete routeRules[rule.name];
				continue;
			}
			if (typeof currentRule.options === "object" && typeof rule.options === "object") currentRule.options = {
				...currentRule.options,
				...rule.options
			};
			else currentRule.options = rule.options;
			currentRule.route = rule.route;
			currentRule.params = {
				...currentRule.params,
				...layer.params
			};
		} else if (rule.options !== false) routeRules[rule.name] = {
			...rule,
			params: layer.params
		};
	}
	const middleware = [];
	const orderedRules = Object.values(routeRules).sort((a, b) => (a.handler?.order || 0) - (b.handler?.order || 0));
	for (const rule of orderedRules) {
		if (rule.options === false || !rule.handler) continue;
		middleware.push(rule.handler(rule));
	}
	return {
		routeRules,
		routeRuleMiddleware: middleware
	};
}
//#endregion
//#region node_modules/nitro/dist/runtime/internal/error/hooks.mjs
function _captureError(error, type) {
	console.error(`[${type}]`, error);
	useNitroApp().captureError?.(error, { tags: [type] });
}
function trapUnhandledErrors() {
	process.on("unhandledRejection", (error) => _captureError(error, "unhandledRejection"));
	process.on("uncaughtException", (error) => _captureError(error, "uncaughtException"));
}
//#endregion
//#region #nitro/virtual/tracing
var tracingSrvxPlugins = [];
//#endregion
//#region node_modules/nitro/dist/presets/node/runtime/node-server.mjs
var _parsedPort = Number.parseInt(process.env.NITRO_PORT ?? process.env.PORT ?? "");
var port = Number.isNaN(_parsedPort) ? 3e3 : _parsedPort;
var host = process.env.NITRO_HOST || process.env.HOST;
var cert = process.env.NITRO_SSL_CERT;
var key = process.env.NITRO_SSL_KEY;
var nitroApp = useNitroApp();
serve({
	port,
	hostname: host,
	tls: cert && key ? {
		cert,
		key
	} : void 0,
	fetch: nitroApp.fetch,
	plugins: [...tracingSrvxPlugins]
});
trapUnhandledErrors();
var node_server_default = {};
//#endregion
export { node_server_default as default };
