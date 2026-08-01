import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { i as formatRelative } from "./GrokLogo-eJjdnOC_.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/RelativeTime-CDg-rQ4W.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
/** Avoids SSR/client time drift hydration mismatches. */
function RelativeTime({ ts, className }) {
	const [label, setLabel] = (0, import_react.useState)("—");
	(0, import_react.useEffect)(() => {
		const update = () => setLabel(formatRelative(ts));
		update();
		const id = window.setInterval(update, 15e3);
		return () => window.clearInterval(id);
	}, [ts]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className,
		children: label
	});
}
//#endregion
export { RelativeTime as t };
