import { n as GROK_PROVIDERS } from "./providers-DD9Wq7fi.mjs";
import { P as require_jsx_runtime, _ as Navigate, h as Link } from "../_libs/@tanstack/react-router+[...].mjs";
import { a as signIn, l as useCurrentUserState, n as GrokHubMark, t as Button } from "./button-Cz9j7Ln5.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/login-DJtZHKJX.js
var import_jsx_runtime = require_jsx_runtime();
function LoginPage() {
	const { user, isPending } = useCurrentUserState();
	if (!isPending && user && !user.isDevFallback) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Navigate, { to: "/" });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("main", {
		className: "grid min-h-dvh place-items-center bg-[var(--color-bg)] p-6 text-[var(--color-fg)]",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "w-full max-w-sm space-y-6",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-col items-center gap-3 text-center",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GrokHubMark, { className: "h-12 w-12" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
						className: "text-xl font-semibold tracking-tight",
						children: "Sign in to GrokHub"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-1 text-sm text-[var(--color-muted)]",
						children: "Connect with your Grok account via the Grok auth broker (Google or X)."
					})] })]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2",
					children: [GROK_PROVIDERS.map((p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						className: "w-full",
						variant: p.idp === "google" ? "default" : "secondary",
						onClick: () => void signIn(p.providerId, { callbackURL: "/" }),
						children: ["Continue with ", p.label]
					}, p.providerId)), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "pt-2 text-center text-xs text-[var(--color-subtle)]",
						children: "OAuth is handled by Grok Build / auth.grok.me — this app never sees Google/X secrets."
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-center text-sm",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Link, {
						to: "/",
						className: "text-[var(--color-muted)] underline-offset-4 hover:underline",
						children: "Continue without account"
					})
				})
			]
		})
	});
}
//#endregion
export { LoginPage as component };
