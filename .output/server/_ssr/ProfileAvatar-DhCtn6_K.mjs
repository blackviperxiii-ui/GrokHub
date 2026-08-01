import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn } from "./GrokLogo-eJjdnOC_.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/ProfileAvatar-DhCtn6_K.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
/** Avatar with automatic initials fallback when URL is missing or fails to load. */
function ProfileAvatar({ src, name, email, className, size = "md" }) {
	const [failed, setFailed] = (0, import_react.useState)(false);
	const label = (name || email || "G").trim();
	const initials = label.split(/\s+/).filter(Boolean).slice(0, 2).map((p) => p[0]?.toUpperCase() || "").join("") || "G";
	const dim = size === "sm" ? "h-8 w-8 text-xs" : size === "lg" ? "h-12 w-12 text-base" : "h-10 w-10 text-sm";
	if (!src || failed) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("grid shrink-0 place-items-center rounded-full bg-[var(--color-elevated)] font-medium text-[var(--color-fg)] ring-1 ring-[var(--color-border)]", dim, className),
		title: label,
		"aria-label": label,
		children: initials
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
		src,
		alt: "",
		title: label,
		className: cn("shrink-0 rounded-full object-cover ring-1 ring-[var(--color-border)]", dim, className),
		referrerPolicy: "no-referrer",
		onError: () => setFailed(true)
	});
}
//#endregion
export { ProfileAvatar as t };
