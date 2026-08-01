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
	"/assets/AgentsView-BVXdufS1.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"cc6-H1K+DB+2bwvMx6zgFV58A9imfto\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3270,
		"path": "../public/assets/AgentsView-BVXdufS1.js"
	},
	"/assets/AutomationsView-DltgrRFh.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"103f-mhbippJpBAM5Zt4H4uswPHeE8pI\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 4159,
		"path": "../public/assets/AutomationsView-DltgrRFh.js"
	},
	"/assets/ChatView-BFLEiwkO.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2a67a-rKGgQux9MVv/0uIbbwyFHy422Y0\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 173690,
		"path": "../public/assets/ChatView-BFLEiwkO.js"
	},
	"/assets/CommandView-CVkCFF20.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1b4a-FhlfWS+fgxtZbwp81HtIjevqXcg\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 6986,
		"path": "../public/assets/CommandView-CVkCFF20.js"
	},
	"/assets/ConnectorsView-DOqbcZbH.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d5f-ztCSOmS1ww7EEZAeKsACtOWuRbM\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3423,
		"path": "../public/assets/ConnectorsView-DOqbcZbH.js"
	},
	"/assets/DesktopHostView-CnKinHYX.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2ae8-Ve8PebnvCn9mXbkw7n1QvOWrHLM\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 10984,
		"path": "../public/assets/DesktopHostView-CnKinHYX.js"
	},
	"/assets/GrokLogo-D_dsCbfo.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"497-DuGx9ic7WwY1oBLTQJekAcB7bcM\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 1175,
		"path": "../public/assets/GrokLogo-D_dsCbfo.js"
	},
	"/assets/HistoryView-BiO8-PFJ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"e3a-XmKZSEOabU8u+dsuJ8lz/fOXe5Y\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3642,
		"path": "../public/assets/HistoryView-BiO8-PFJ.js"
	},
	"/assets/HostGatewayBanner-AvlhmzwF.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"18b9-8Blhrxd+wepAttvW2GQJ59tJoAo\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 6329,
		"path": "../public/assets/HostGatewayBanner-AvlhmzwF.js"
	},
	"/assets/ImagineView-N8KVbAtj.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2d4b-d4B3JVlOJnXc5H9JNt2B6ysrgQQ\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 11595,
		"path": "../public/assets/ImagineView-N8KVbAtj.js"
	},
	"/assets/ProfileAvatar-DtjRjG9c.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"32e-z9NdnWEp4/2asq3UF58tiyhSeS0\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 814,
		"path": "../public/assets/ProfileAvatar-DtjRjG9c.js"
	},
	"/assets/RelativeTime-tF303D4T.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"165-xSjpGdAAcbO3mFS7S8z775hH7vs\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 357,
		"path": "../public/assets/RelativeTime-tF303D4T.js"
	},
	"/assets/SettingsView-C4zdam-D.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"4658-+iw6jEZaiCQxBlBGqDmyX5Qhsow\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 18008,
		"path": "../public/assets/SettingsView-C4zdam-D.js"
	},
	"/assets/SkillsView-3Sc-Sxaz.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1318-/Ju757x7nDRt6JaJenp3U9Szhiw\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 4888,
		"path": "../public/assets/SkillsView-3Sc-Sxaz.js"
	},
	"/assets/UsageMeter-C8ptaMjb.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1d6e-oyKyxMwZFpJcHIJVstktHw8Bvco\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 7534,
		"path": "../public/assets/UsageMeter-C8ptaMjb.js"
	},
	"/assets/automation-schedule-CxNhus6_.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"3c1-YP+1AoGh2pMNInxr9Dt+s3wLOgs\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 961,
		"path": "../public/assets/automation-schedule-CxNhus6_.js"
	},
	"/assets/button-PTui52Sr.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"51c-N3/mqHNn45iABnC8wcg2FW+cfug\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 1308,
		"path": "../public/assets/button-PTui52Sr.js"
	},
	"/assets/cable-C9U5_Tnj.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1a0-/QbWmwQpd4ubOaY4wzEqt7HI7Vw\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 416,
		"path": "../public/assets/cable-C9U5_Tnj.js"
	},
	"/assets/card-DtLXjuNl.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"11070-8rBIJtx6620jkHlpc7PBL9Vi79k\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 69744,
		"path": "../public/assets/card-DtLXjuNl.js"
	},
	"/assets/connector-tools-BqZf6rh-.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"13eb-M4bYsna7BbfPOO6nEFmVLFcqS+k\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 5099,
		"path": "../public/assets/connector-tools-BqZf6rh-.js"
	},
	"/assets/dist-CLNNfRwf.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"6e94-GgJ+wN0kekbnNp3bqPoZahoBo2Q\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 28308,
		"path": "../public/assets/dist-CLNNfRwf.js"
	},
	"/assets/folder-input-BYXvpljT.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"12e-Gelr6i+45bAW4yo1B3xSClr4Rrw\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 302,
		"path": "../public/assets/folder-input-BYXvpljT.js"
	},
	"/assets/gauge-B_6rsDMc.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"a4-qU4B+C4pxLRADxmUpyLIXWjGD14\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 164,
		"path": "../public/assets/gauge-B_6rsDMc.js"
	},
	"/assets/grok-BSniE9QE.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"824-6x1uUkTkTIhMfpwO5C4pjBN3WPI\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 2084,
		"path": "../public/assets/grok-BSniE9QE.js"
	},
	"/assets/grok-client-DHaxRp7l.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dce-Si0Oe4fW/D0IXkzuDisdCguf/5M\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3534,
		"path": "../public/assets/grok-client-DHaxRp7l.js"
	},
	"/assets/grok-website-usage-Dqpi-Tiv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1caa-IHUMAs9KQb+sXyPIgKQjGqxYp7o\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 7338,
		"path": "../public/assets/grok-website-usage-Dqpi-Tiv.js"
	},
	"/assets/hard-drive-CtZA7F_t.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"17b-o8QtW+MeHLPfPCsrlhatCEcFcac\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 379,
		"path": "../public/assets/hard-drive-CtZA7F_t.js"
	},
	"/assets/host-client-BqP38civ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"925-Vsdw6E6cdholAWaa/woxQMk+yZQ\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 2341,
		"path": "../public/assets/host-client-BqP38civ.js"
	},
	"/assets/host-safety-F25AUCFv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"64a-SpuiaxmBInIV0k2ISOAF+XXdA1A\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 1610,
		"path": "../public/assets/host-safety-F25AUCFv.js"
	},
	"/assets/image-D2yWg6V9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-32JonNTF0e5LXq1LDuwqUHrbibM\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 257,
		"path": "../public/assets/image-D2yWg6V9.js"
	},
	"/assets/index-DVkSgRIX.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"4a9d1-mparm2/M9lV67lQTpI6HrJcLego\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 305617,
		"path": "../public/assets/index-DVkSgRIX.js"
	},
	"/assets/input-stfai6_O.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-bdJHDEhj588XZ8GpznevfxMMht8\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 581,
		"path": "../public/assets/input-stfai6_O.js"
	},
	"/assets/jsx-runtime-KJkY8l8U.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2157-uh2PnvJKYWZAlieFni6eRY8YAVs\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 8535,
		"path": "../public/assets/jsx-runtime-KJkY8l8U.js"
	},
	"/assets/loader-circle-CMevi66R.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"84-Tp4zLTsj2gnUQqn5X1wFFgViqjA\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 132,
		"path": "../public/assets/loader-circle-CMevi66R.js"
	},
	"/assets/login-C-COaalc.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"69b-CYtsabn5KnCrv5Ub2pCYRRhFMlU\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 1691,
		"path": "../public/assets/login-C-COaalc.js"
	},
	"/assets/message-square-plus-D0DRShM_.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f4-MBZZ3FDyR054Dirooz8hYfCDOJg\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 244,
		"path": "../public/assets/message-square-plus-D0DRShM_.js"
	},
	"/assets/openclaw-import-CRJUnD2p.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f7a-2XiG/hFckln0bhpCVuqz3wahUq8\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3962,
		"path": "../public/assets/openclaw-import-CRJUnD2p.js"
	},
	"/assets/play-C5_g0l9m.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"7a-tiRaf+brlfGO+xdcueNIA/RS/R4\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 122,
		"path": "../public/assets/play-C5_g0l9m.js"
	},
	"/assets/plus-K3x5YPXx.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8d-mrRAgfq0C8Ct1OJw95mFLXx5PtM\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 141,
		"path": "../public/assets/plus-K3x5YPXx.js"
	},
	"/assets/react-dom-BsrxZfdi.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dda-/KkbQdWpmEcddLNsaiMx/D5PdtI\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3546,
		"path": "../public/assets/react-dom-BsrxZfdi.js"
	},
	"/assets/refresh-cw-CJtOzG1O.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"135-E3fbN9gIRW68lYQpHvOvdTxXNHQ\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 309,
		"path": "../public/assets/refresh-cw-CJtOzG1O.js"
	},
	"/assets/routes-DjqhjgeR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"5d47-l0LzGgB7DCshoGOoQNd+korOGaA\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 23879,
		"path": "../public/assets/routes-DjqhjgeR.js"
	},
	"/assets/secrets-client-BSOFMExG.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"257-lWqWk5xSkICzrvrvNCTDhqN731k\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 599,
		"path": "../public/assets/secrets-client-BSOFMExG.js"
	},
	"/assets/seed-D6kskcO0.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1219-LxXM1Qalc8if10ZfEX3y2HZY+dQ\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 4633,
		"path": "../public/assets/seed-D6kskcO0.js"
	},
	"/assets/sparkles-ZqDwQcQQ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1f0-CrdsQtkYG/6uCQ3kDOJdc8CRPE4\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 496,
		"path": "../public/assets/sparkles-ZqDwQcQQ.js"
	},
	"/assets/square-B-0fX8vO.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"87-SwubJ7YDtwkpBvi0TDyCC1xjK2M\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 135,
		"path": "../public/assets/square-B-0fX8vO.js"
	},
	"/assets/styles-DkwIQoJI.css": {
		"type": "text/css; charset=utf-8",
		"etag": "\"c7aa-8B1aUhOjnQuMsfvV9jGszt+mbak\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 51114,
		"path": "../public/assets/styles-DkwIQoJI.css"
	},
	"/assets/terminal-CLQ6PQrt.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"b3-iU0RQ7Ub2G8JSA7zlPypQlDblCs\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 179,
		"path": "../public/assets/terminal-CLQ6PQrt.js"
	},
	"/assets/textarea-CzzBL_GL.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-21uDSBgHx4/9QT/pvTZA0Ucssdk\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 581,
		"path": "../public/assets/textarea-CzzBL_GL.js"
	},
	"/assets/timer-reset-Bx2xvgsk.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-5vrcLBUSKJ+dRVoGrFoyfDWLuVY\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 257,
		"path": "../public/assets/timer-reset-Bx2xvgsk.js"
	},
	"/assets/trash-2-BpILat5l.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2e7-8AFSwzlvM9QLwT9ljzEtI1RuNv4\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 743,
		"path": "../public/assets/trash-2-BpILat5l.js"
	},
	"/assets/use-current-user-BDqHjrBD.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"79be-4sntSCUxL9dv+F+8iqvBAPDoAhU\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 31166,
		"path": "../public/assets/use-current-user-BDqHjrBD.js"
	},
	"/assets/users-CJvAc0h8.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"126-5ple1Ni36FA+lyw9oeO7GdeWu/I\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 294,
		"path": "../public/assets/users-CJvAc0h8.js"
	},
	"/assets/website-connectors-BXiXQhqJ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d13-Rf1m+7AyHyk4uHV/V/ZBdo2pPjk\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 3347,
		"path": "../public/assets/website-connectors-BXiXQhqJ.js"
	},
	"/assets/x-DZEbZXva.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8e-JOTWLKIUcnkMaNYq6OI0n8zTyYE\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 142,
		"path": "../public/assets/x-DZEbZXva.js"
	},
	"/assets/zap-C-_rImZR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"fa-xDvwB/PbBzGzSEH1Oi12r4eU1bw\"",
		"mtime": "2026-08-01T23:16:40.213Z",
		"size": 250,
		"path": "../public/assets/zap-C-_rImZR.js"
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
