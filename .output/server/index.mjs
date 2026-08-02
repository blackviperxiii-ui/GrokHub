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
	"/assets/AgentsView-BQCY9o46.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"cc6-QJ922s3j3i27gHcJOE9sPazHPd0\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3270,
		"path": "../public/assets/AgentsView-BQCY9o46.js"
	},
	"/assets/AutomationsView-CNn2CvN9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1c09-JLZJrsOYPhfnK/+fNSMpwaHE23c\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 7177,
		"path": "../public/assets/AutomationsView-CNn2CvN9.js"
	},
	"/assets/ChatView-Cfi2qGsM.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2a67a-OhtrKcLYvzJGZzd/lY5bYj9OVKA\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 173690,
		"path": "../public/assets/ChatView-Cfi2qGsM.js"
	},
	"/assets/CommandView-DOdWcr5n.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1b45-VwnquY4sAKyTy/BQr9AnISWvG5o\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 6981,
		"path": "../public/assets/CommandView-DOdWcr5n.js"
	},
	"/assets/ConnectorsView-avf84iak.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d69-jKeag9+j3T+jvjCC9CKNGoTURXk\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3433,
		"path": "../public/assets/ConnectorsView-avf84iak.js"
	},
	"/assets/DesktopHostView-Du6AZuj7.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2ae3-YVpWIn4cidQqgDOb0UQax2XtwW8\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 10979,
		"path": "../public/assets/DesktopHostView-Du6AZuj7.js"
	},
	"/assets/GrokLogo-D_dsCbfo.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"497-DuGx9ic7WwY1oBLTQJekAcB7bcM\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 1175,
		"path": "../public/assets/GrokLogo-D_dsCbfo.js"
	},
	"/assets/HistoryView-CB8Pj1nR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"e3a-pvH8k0FfBmY+mx2aceg7AlAZ0xQ\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3642,
		"path": "../public/assets/HistoryView-CB8Pj1nR.js"
	},
	"/assets/HostGatewayBanner-BGhyMxqm.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"18b4-PaySMf6U4agx68d/a0SzCmuf0Ac\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 6324,
		"path": "../public/assets/HostGatewayBanner-BGhyMxqm.js"
	},
	"/assets/ImagineView-8T99KkZy.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2d4b-ELEpifVvkvPY127TA5nClcRbKXk\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 11595,
		"path": "../public/assets/ImagineView-8T99KkZy.js"
	},
	"/assets/ProfileAvatar-DtjRjG9c.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"32e-z9NdnWEp4/2asq3UF58tiyhSeS0\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 814,
		"path": "../public/assets/ProfileAvatar-DtjRjG9c.js"
	},
	"/assets/RelativeTime-tF303D4T.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"165-xSjpGdAAcbO3mFS7S8z775hH7vs\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 357,
		"path": "../public/assets/RelativeTime-tF303D4T.js"
	},
	"/assets/SettingsView-D4F9peW8.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"7639-xfN5FCi2/VbNlUsr/XUOxHYa6AI\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 30265,
		"path": "../public/assets/SettingsView-D4F9peW8.js"
	},
	"/assets/SkillsView-fQhx1frR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1318-Wr/y60okmfkTeYKM822nLIi49Bg\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 4888,
		"path": "../public/assets/SkillsView-fQhx1frR.js"
	},
	"/assets/UsageMeter-BQOoJD8Z.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1ed7-q++EOPEJyvYR3BbNd3nRYniYFS0\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 7895,
		"path": "../public/assets/UsageMeter-BQOoJD8Z.js"
	},
	"/assets/automation-schedule-JledrSXh.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8fb-ydJlAZQ8foEY4A/WmnCmNbrsIyE\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 2299,
		"path": "../public/assets/automation-schedule-JledrSXh.js"
	},
	"/assets/button-PTui52Sr.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"51c-N3/mqHNn45iABnC8wcg2FW+cfug\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 1308,
		"path": "../public/assets/button-PTui52Sr.js"
	},
	"/assets/cable-CiqeSXXB.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1a0-5ZMtiSvlL6OsoKs4mLnwyBXPpYI\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 416,
		"path": "../public/assets/cable-CiqeSXXB.js"
	},
	"/assets/card-Bzgfa4jV.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"150ef-6iU/TZEctdE69ao4vByMlT+co5w\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 86255,
		"path": "../public/assets/card-Bzgfa4jV.js"
	},
	"/assets/connector-tools-BqZf6rh-.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"13eb-M4bYsna7BbfPOO6nEFmVLFcqS+k\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 5099,
		"path": "../public/assets/connector-tools-BqZf6rh-.js"
	},
	"/assets/dist-CLNNfRwf.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"6e94-GgJ+wN0kekbnNp3bqPoZahoBo2Q\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 28308,
		"path": "../public/assets/dist-CLNNfRwf.js"
	},
	"/assets/folder-input-BtBUJt23.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"12e-THCKjXN54xO8TZUzXEKhiV0svJA\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 302,
		"path": "../public/assets/folder-input-BtBUJt23.js"
	},
	"/assets/gauge-CvkJE2l6.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"a4-ejE7DQCivnc1pqfR0chXCiMnfHs\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 164,
		"path": "../public/assets/gauge-CvkJE2l6.js"
	},
	"/assets/grok-BSniE9QE.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"824-6x1uUkTkTIhMfpwO5C4pjBN3WPI\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 2084,
		"path": "../public/assets/grok-BSniE9QE.js"
	},
	"/assets/grok-client-DHaxRp7l.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dce-Si0Oe4fW/D0IXkzuDisdCguf/5M\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3534,
		"path": "../public/assets/grok-client-DHaxRp7l.js"
	},
	"/assets/hard-drive-w5lAFSTg.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"17b-s+9Uj6Z5DPw5S3khBSIfDR4NFFA\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 379,
		"path": "../public/assets/hard-drive-w5lAFSTg.js"
	},
	"/assets/host-client-BqP38civ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"925-Vsdw6E6cdholAWaa/woxQMk+yZQ\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 2341,
		"path": "../public/assets/host-client-BqP38civ.js"
	},
	"/assets/host-safety-F25AUCFv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"64a-SpuiaxmBInIV0k2ISOAF+XXdA1A\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 1610,
		"path": "../public/assets/host-safety-F25AUCFv.js"
	},
	"/assets/image-BlyZw7az.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-FGdfgdzTiFlvBGRqLmHtL0t7AhY\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 257,
		"path": "../public/assets/image-BlyZw7az.js"
	},
	"/assets/grok-website-usage-Dqpi-Tiv.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1caa-IHUMAs9KQb+sXyPIgKQjGqxYp7o\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 7338,
		"path": "../public/assets/grok-website-usage-Dqpi-Tiv.js"
	},
	"/assets/index-AD8cz4kR.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"4a9d1-JnqjIHx/Pn3XyEJ1steCgtOSeIM\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 305617,
		"path": "../public/assets/index-AD8cz4kR.js"
	},
	"/assets/input-stfai6_O.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-bdJHDEhj588XZ8GpznevfxMMht8\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 581,
		"path": "../public/assets/input-stfai6_O.js"
	},
	"/assets/jsx-runtime-KJkY8l8U.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2157-uh2PnvJKYWZAlieFni6eRY8YAVs\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 8535,
		"path": "../public/assets/jsx-runtime-KJkY8l8U.js"
	},
	"/assets/loader-circle-OKcPgaI9.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"84-P+aCdC47I9qmw4UBX9It42qesFI\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 132,
		"path": "../public/assets/loader-circle-OKcPgaI9.js"
	},
	"/assets/login-BN3GR-2o.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"69b-qFlCTw9eg2EOI0FSB3IuaC4Hkt4\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 1691,
		"path": "../public/assets/login-BN3GR-2o.js"
	},
	"/assets/message-square-plus-BPj9u-IP.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f4-hed/nbB5CjzzaYS9iSa7lQ62Dh0\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 244,
		"path": "../public/assets/message-square-plus-BPj9u-IP.js"
	},
	"/assets/openclaw-import-CRJUnD2p.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"f7a-2XiG/hFckln0bhpCVuqz3wahUq8\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3962,
		"path": "../public/assets/openclaw-import-CRJUnD2p.js"
	},
	"/assets/play-C4upn0VH.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"7a-xUgfYozce3dp3T2QUsudteQ9190\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 122,
		"path": "../public/assets/play-C4upn0VH.js"
	},
	"/assets/plus-Cy1--Vnl.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8d-dvHzgAuUJjRTVRHJKSpe85lyTSw\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 141,
		"path": "../public/assets/plus-Cy1--Vnl.js"
	},
	"/assets/react-dom-BsrxZfdi.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"dda-/KkbQdWpmEcddLNsaiMx/D5PdtI\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3546,
		"path": "../public/assets/react-dom-BsrxZfdi.js"
	},
	"/assets/refresh-cw-Q26LKTYu.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"135-3K1knNmBdoCHsY6coDgjnbRlccY\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 309,
		"path": "../public/assets/refresh-cw-Q26LKTYu.js"
	},
	"/assets/routes-k4DXxw8D.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"5d64-h66n3ZahItztKxQukSjuW2LDoe8\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 23908,
		"path": "../public/assets/routes-k4DXxw8D.js"
	},
	"/assets/secrets-client-BSOFMExG.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"257-lWqWk5xSkICzrvrvNCTDhqN731k\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 599,
		"path": "../public/assets/secrets-client-BSOFMExG.js"
	},
	"/assets/seed-D6kskcO0.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1219-LxXM1Qalc8if10ZfEX3y2HZY+dQ\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 4633,
		"path": "../public/assets/seed-D6kskcO0.js"
	},
	"/assets/self-mod-client-70I45E9v.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"9f8-OYzNmyXj22kt4kErWfEIkVriigY\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 2552,
		"path": "../public/assets/self-mod-client-70I45E9v.js"
	},
	"/assets/setup-crypto-DAgaZZ3P.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"56c-rX6wNjZwFSc2rtNUZNsdiorB1Vg\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 1388,
		"path": "../public/assets/setup-crypto-DAgaZZ3P.js"
	},
	"/assets/sparkles-D8JvNMrQ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"1f0-+Nx9ZHvX/R3KbL6fS/rAaEYNjKo\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 496,
		"path": "../public/assets/sparkles-D8JvNMrQ.js"
	},
	"/assets/square-ebbYveiM.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"87-c5dwUzcX+HW8jJVOXFMf6ytCUKk\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 135,
		"path": "../public/assets/square-ebbYveiM.js"
	},
	"/assets/styles-DkYEutiK.css": {
		"type": "text/css; charset=utf-8",
		"etag": "\"c855-7iQuJE8qaQFByxemHUBZlJfh8qU\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 51285,
		"path": "../public/assets/styles-DkYEutiK.css"
	},
	"/assets/terminal-DIflPS9F.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"b3-IZ6FiEnmtqfmKwaMk2E8v5JiUSY\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 179,
		"path": "../public/assets/terminal-DIflPS9F.js"
	},
	"/assets/timer-reset-dLa0MNjM.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"101-zlAHIe7U8ixbmaMZfUDK5ECU+Co\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 257,
		"path": "../public/assets/timer-reset-dLa0MNjM.js"
	},
	"/assets/textarea-CzzBL_GL.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"245-21uDSBgHx4/9QT/pvTZA0Ucssdk\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 581,
		"path": "../public/assets/textarea-CzzBL_GL.js"
	},
	"/assets/use-current-user-BDqHjrBD.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"79be-4sntSCUxL9dv+F+8iqvBAPDoAhU\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 31166,
		"path": "../public/assets/use-current-user-BDqHjrBD.js"
	},
	"/assets/trash-2-CLgKAtXh.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"2e7-tBGiVza2/F681u0FpWdOy+pmJVM\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 743,
		"path": "../public/assets/trash-2-CLgKAtXh.js"
	},
	"/assets/users-DCzl-LiX.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"126-cpq4IoBpBQjr0SBWTTPQS+yEMoo\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 294,
		"path": "../public/assets/users-DCzl-LiX.js"
	},
	"/assets/website-connectors-BXiXQhqJ.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"d13-Rf1m+7AyHyk4uHV/V/ZBdo2pPjk\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 3347,
		"path": "../public/assets/website-connectors-BXiXQhqJ.js"
	},
	"/assets/x-BcXQ25qp.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"8e-622p8+2k3YtVnm5mJ2PHddWShGQ\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 142,
		"path": "../public/assets/x-BcXQ25qp.js"
	},
	"/assets/zap-CrkIgnXq.js": {
		"type": "text/javascript; charset=utf-8",
		"etag": "\"fa-Bx40vwEeybInT4FPAQvmsg927FM\"",
		"mtime": "2026-08-02T02:18:44.478Z",
		"size": 250,
		"path": "../public/assets/zap-CrkIgnXq.js"
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
