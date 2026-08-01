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
	"/assets/AgentsView-CNgCVolx.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"cc6-OrwYGstBnH47s08At7ATia1LJyA\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 3270,
		"path": "../public/assets/AgentsView-CNgCVolx.js"
	},
	"/assets/AutomationsView-BDySG41F.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"103f-4Qc132rP4BdxoNmYUDlOUkaHFZ4\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 4159,
		"path": "../public/assets/AutomationsView-BDySG41F.js"
	},
	"/assets/ChatView-B926DFT8.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2a67a-ICBbfOMY1cYQ32QIhlYp80dV0lc\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 173690,
		"path": "../public/assets/ChatView-B926DFT8.js"
	},
	"/assets/CommandView-2TU0bvPa.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1b4a-h48JcTBqtSRwVZWNeBq0vyOkgNc\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 6986,
		"path": "../public/assets/CommandView-2TU0bvPa.js"
	},
	"/assets/ConnectorsView-ZVRxXiqE.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d5f-zVBFCVQhLjXg7Il6/y9s0RoL1eY\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 3423,
		"path": "../public/assets/ConnectorsView-ZVRxXiqE.js"
	},
	"/assets/DesktopHostView-SUa-5_uw.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2ae8-leCUjAQrIeFtqhW30zU/aMYAey0\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 10984,
		"path": "../public/assets/DesktopHostView-SUa-5_uw.js"
	},
	"/assets/GrokLogo-D_dsCbfo.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"497-DuGx9ic7WwY1oBLTQJekAcB7bcM\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 1175,
		"path": "../public/assets/GrokLogo-D_dsCbfo.js"
	},
	"/assets/HistoryView-DGukivel.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"e3a-8ONKzon83EuGeRPi4hzH3hqB+1g\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 3642,
		"path": "../public/assets/HistoryView-DGukivel.js"
	},
	"/assets/HostGatewayBanner-DCH2B9pu.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"18b9-DfmL8j7V8NhSvn04J2aOq9QoJ98\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 6329,
		"path": "../public/assets/HostGatewayBanner-DCH2B9pu.js"
	},
	"/assets/ImagineView-Cz8GI6iJ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2d4b-p4ChrjNjpoOXcElcEe7G3RdwV1k\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 11595,
		"path": "../public/assets/ImagineView-Cz8GI6iJ.js"
	},
	"/assets/ProfileAvatar-DtjRjG9c.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"32e-z9NdnWEp4/2asq3UF58tiyhSeS0\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 814,
		"path": "../public/assets/ProfileAvatar-DtjRjG9c.js"
	},
	"/assets/RelativeTime-tF303D4T.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"165-xSjpGdAAcbO3mFS7S8z775hH7vs\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 357,
		"path": "../public/assets/RelativeTime-tF303D4T.js"
	},
	"/assets/SettingsView-DqN2F9T9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"5460-4N7YYDtj6X22v5+SgKHKFmILzcA\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 21600,
		"path": "../public/assets/SettingsView-DqN2F9T9.js"
	},
	"/assets/SkillsView-CrZpLSq8.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1318-SITNx0V7ETb2VhuQY2b/rYqn1TE\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 4888,
		"path": "../public/assets/SkillsView-CrZpLSq8.js"
	},
	"/assets/UsageMeter-D2gjsNBp.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1ed7-36Sc2CKZjhZ4yldMIQqOPqpYsUY\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 7895,
		"path": "../public/assets/UsageMeter-D2gjsNBp.js"
	},
	"/assets/automation-schedule-CxNhus6_.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"3c1-YP+1AoGh2pMNInxr9Dt+s3wLOgs\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 961,
		"path": "../public/assets/automation-schedule-CxNhus6_.js"
	},
	"/assets/button-PTui52Sr.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"51c-N3/mqHNn45iABnC8wcg2FW+cfug\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 1308,
		"path": "../public/assets/button-PTui52Sr.js"
	},
	"/assets/cable-CQ0mkgcA.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1a0-49GARSAmWVoRYh8eV5ceyaddplc\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 416,
		"path": "../public/assets/cable-CQ0mkgcA.js"
	},
	"/assets/card-BH5DkVNa.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"11c35-6rTkzXFH04GoAiX5BlH9s/C7NWU\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 72757,
		"path": "../public/assets/card-BH5DkVNa.js"
	},
	"/assets/connector-tools-BqZf6rh-.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"13eb-M4bYsna7BbfPOO6nEFmVLFcqS+k\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 5099,
		"path": "../public/assets/connector-tools-BqZf6rh-.js"
	},
	"/assets/dist-CLNNfRwf.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"6e94-GgJ+wN0kekbnNp3bqPoZahoBo2Q\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 28308,
		"path": "../public/assets/dist-CLNNfRwf.js"
	},
	"/assets/folder-input-BRqlEgVk.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"12e-zpm70+Zbh9dNYw+rVtoLR4EleTU\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 302,
		"path": "../public/assets/folder-input-BRqlEgVk.js"
	},
	"/assets/gauge-AsPBAEFX.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"a4-eobYwnHMqc73iuONPzXbS3YZMWo\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 164,
		"path": "../public/assets/gauge-AsPBAEFX.js"
	},
	"/assets/grok-BSniE9QE.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"824-6x1uUkTkTIhMfpwO5C4pjBN3WPI\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 2084,
		"path": "../public/assets/grok-BSniE9QE.js"
	},
	"/assets/grok-client-DHaxRp7l.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dce-Si0Oe4fW/D0IXkzuDisdCguf/5M\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 3534,
		"path": "../public/assets/grok-client-DHaxRp7l.js"
	},
	"/assets/grok-website-usage-Dqpi-Tiv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1caa-IHUMAs9KQb+sXyPIgKQjGqxYp7o\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 7338,
		"path": "../public/assets/grok-website-usage-Dqpi-Tiv.js"
	},
	"/assets/hard-drive-BF8-vfvh.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"17b-dFzd5TAI+orGq2MhGbeSYi2naBk\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 379,
		"path": "../public/assets/hard-drive-BF8-vfvh.js"
	},
	"/assets/host-client-BqP38civ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"925-Vsdw6E6cdholAWaa/woxQMk+yZQ\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 2341,
		"path": "../public/assets/host-client-BqP38civ.js"
	},
	"/assets/host-safety-F25AUCFv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"64a-SpuiaxmBInIV0k2ISOAF+XXdA1A\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 1610,
		"path": "../public/assets/host-safety-F25AUCFv.js"
	},
	"/assets/image-Dc89IUca.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-2OXZOWTGkDJ2kVWR9/psi4EBBeg\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 257,
		"path": "../public/assets/image-Dc89IUca.js"
	},
	"/assets/index-C0pUVns6.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"4a9d1-MplzaYKb8pTpwEMwJ2IkxMY/nvQ\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 305617,
		"path": "../public/assets/index-C0pUVns6.js"
	},
	"/assets/input-stfai6_O.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-bdJHDEhj588XZ8GpznevfxMMht8\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 581,
		"path": "../public/assets/input-stfai6_O.js"
	},
	"/assets/jsx-runtime-KJkY8l8U.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2157-uh2PnvJKYWZAlieFni6eRY8YAVs\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 8535,
		"path": "../public/assets/jsx-runtime-KJkY8l8U.js"
	},
	"/assets/loader-circle-DqT0hMAz.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"84-ItJlUBDP9O1T8xQkgULczogkxxs\"",
		"mtime": "2026-08-01T23:49:34.461Z",
		"size": 132,
		"path": "../public/assets/loader-circle-DqT0hMAz.js"
	},
	"/assets/login-dKkfeFPp.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"69b-qqSc1StXl8a+7gBIvtgSMWd/A78\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 1691,
		"path": "../public/assets/login-dKkfeFPp.js"
	},
	"/assets/message-square-plus-CoC5dSR_.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f4-RVtlQhe0goF1JCCNpLX6+HmcZ4k\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 244,
		"path": "../public/assets/message-square-plus-CoC5dSR_.js"
	},
	"/assets/openclaw-import-CRJUnD2p.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f7a-2XiG/hFckln0bhpCVuqz3wahUq8\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 3962,
		"path": "../public/assets/openclaw-import-CRJUnD2p.js"
	},
	"/assets/play-CKs7f7B-.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"7a-E1IIxiS+z46csvWf2EqwgkkALig\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 122,
		"path": "../public/assets/play-CKs7f7B-.js"
	},
	"/assets/plus-_yWiX0DN.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8d-Q4arynxuVTDc7tnyACfzxfymG6E\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 141,
		"path": "../public/assets/plus-_yWiX0DN.js"
	},
	"/assets/react-dom-BsrxZfdi.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dda-/KkbQdWpmEcddLNsaiMx/D5PdtI\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 3546,
		"path": "../public/assets/react-dom-BsrxZfdi.js"
	},
	"/assets/refresh-cw-Cnv4SnNG.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"135-i52JV9h0n6AyvsifYglHaZJ4Ui4\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 309,
		"path": "../public/assets/refresh-cw-Cnv4SnNG.js"
	},
	"/assets/routes-CNZQZFol.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"5d6f-VA44OWeiXZXCJAaHbD5MaAqHga4\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 23919,
		"path": "../public/assets/routes-CNZQZFol.js"
	},
	"/assets/secrets-client-BSOFMExG.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"257-lWqWk5xSkICzrvrvNCTDhqN731k\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 599,
		"path": "../public/assets/secrets-client-BSOFMExG.js"
	},
	"/assets/seed-D6kskcO0.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1219-LxXM1Qalc8if10ZfEX3y2HZY+dQ\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 4633,
		"path": "../public/assets/seed-D6kskcO0.js"
	},
	"/assets/self-mod-client-70I45E9v.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"9f8-OYzNmyXj22kt4kErWfEIkVriigY\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 2552,
		"path": "../public/assets/self-mod-client-70I45E9v.js"
	},
	"/assets/sparkles-I5cpgcNA.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1f0-wNer/hMPSBZg10seAu4nIa/R9pg\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 496,
		"path": "../public/assets/sparkles-I5cpgcNA.js"
	},
	"/assets/square-D0gIjVn9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"87-k+brnNgU7kAbDaCZL7UkPoI48L0\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 135,
		"path": "../public/assets/square-D0gIjVn9.js"
	},
	"/assets/styles-DkwIQoJI.css": {
		"type": "text/css; charset=utf-8",
		"etag": "\"c7aa-8B1aUhOjnQuMsfvV9jGszt+mbak\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 51114,
		"path": "../public/assets/styles-DkwIQoJI.css"
	},
	"/assets/terminal-BDu7Ds7d.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"b3-SGrR0+CdP5rszXD74zxjn2oKECs\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 179,
		"path": "../public/assets/terminal-BDu7Ds7d.js"
	},
	"/assets/textarea-CzzBL_GL.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-21uDSBgHx4/9QT/pvTZA0Ucssdk\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 581,
		"path": "../public/assets/textarea-CzzBL_GL.js"
	},
	"/assets/timer-reset-CNnGPy82.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-jXvR/gllLRcAte3/pO4DVjOD9wQ\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 257,
		"path": "../public/assets/timer-reset-CNnGPy82.js"
	},
	"/assets/trash-2-fErsnQ_m.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2e7-CMJ9j2TL24iMzWuD5qSWR18eiIw\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 743,
		"path": "../public/assets/trash-2-fErsnQ_m.js"
	},
	"/assets/use-current-user-BDqHjrBD.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"79be-4sntSCUxL9dv+F+8iqvBAPDoAhU\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 31166,
		"path": "../public/assets/use-current-user-BDqHjrBD.js"
	},
	"/assets/users-CpK_P87s.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"126-FeypBNntnFT1bbSChC1OnCzIEBs\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 294,
		"path": "../public/assets/users-CpK_P87s.js"
	},
	"/assets/website-connectors-BXiXQhqJ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d13-Rf1m+7AyHyk4uHV/V/ZBdo2pPjk\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 3347,
		"path": "../public/assets/website-connectors-BXiXQhqJ.js"
	},
	"/assets/x-BL0ineRf.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8e-841qCRGEBROn7ppn7zvWVSgDjn8\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 142,
		"path": "../public/assets/x-BL0ineRf.js"
	},
	"/assets/zap-DqYTUEa6.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"fa-FewBwbQEVHDI558Wji6dZp2nVj0\"",
		"mtime": "2026-08-01T23:49:34.465Z",
		"size": 250,
		"path": "../public/assets/zap-DqYTUEa6.js"
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
