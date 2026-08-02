import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dgqzm_EC.mjs";
import { E as LoaderCircle, M as HardDrive, d as ShieldAlert, g as Plug, m as RefreshCw, u as ShieldCheck } from "../_libs/lucide-react.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/HostGatewayBanner-DTFXC7I1.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
/**
* Desktop host / gateway connector.
* When the unsandboxed bridge is offline, this is the CTA to reconnect so Grok
* can run shell, files, and apps on the user's machine.
*/
function HostGatewayBanner({ variant = "card", className, onOpenDesktop }) {
	const setNav = useGrokHub((s) => s.setNav);
	const connectConnector = useGrokHub((s) => s.connectConnector);
	const [info, setInfo] = (0, import_react.useState)(null);
	const [error, setError] = (0, import_react.useState)(null);
	const [busy, setBusy] = (0, import_react.useState)(false);
	const [isElectron, setIsElectron] = (0, import_react.useState)(false);
	const probe = (0, import_react.useCallback)(async () => {
		setBusy(true);
		setError(null);
		try {
			const mod = await import("./host-client-D2zxhrZc.mjs");
			setIsElectron(mod.isDesktopShell());
			const i = await mod.hostInfo();
			setInfo(i);
			if (i.bridge !== "none" && i.unsandboxed) useGrokHub.setState((s) => ({ connectors: s.connectors.map((c) => c.id === "desktop-host" ? {
				...c,
				status: "connected",
				lastUsed: Date.now()
			} : c) }));
			return i;
		} catch (e) {
			const msg = e instanceof Error ? e.message : "Host probe failed";
			setError(msg);
			setInfo(null);
			return null;
		} finally {
			setBusy(false);
		}
	}, [connectConnector]);
	(0, import_react.useEffect)(() => {
		probe();
	}, [probe]);
	const online = Boolean(info && info.bridge !== "none" && info.unsandboxed);
	async function connectGateway() {
		setBusy(true);
		setError(null);
		try {
			const i = await probe();
			if (i && i.bridge !== "none" && i.unsandboxed) {
				useGrokHub.setState((s) => ({ connectors: s.connectors.map((c) => c.id === "desktop-host" ? {
					...c,
					status: "connected",
					lastUsed: Date.now()
				} : c) }));
				return;
			}
			if (typeof window !== "undefined" && window.grokhubDesktop) setError("Electron shell is present but host IPC failed. Fully quit GrokHub (tray too) and relaunch.");
			else setError("No desktop gateway in this window. Launch the Arch package: grokhub (Electron). Web-only preview cannot control your machine.");
		} finally {
			setBusy(false);
		}
	}
	if (variant === "compact") {
		if (online) return null;
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn("flex flex-wrap items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_45%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2", className),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-2 text-xs text-[var(--color-warn)]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldAlert, { className: "h-3.5 w-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate",
					children: "Desktop gateway offline — agent cannot run shell/files on your machine."
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex shrink-0 gap-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					size: "sm",
					variant: "secondary",
					disabled: busy,
					onClick: () => void connectGateway(),
					children: [busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-3.5 w-3.5 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plug, { className: "h-3.5 w-3.5" }), "Connect host"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					variant: "ghost",
					onClick: () => {
						setNav("desktop");
						onOpenDesktop?.();
					},
					children: "Desktop"
				})]
			})]
		});
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
		className,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap items-start justify-between gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HardDrive, { className: "h-4 w-4" }), "Desktop host gateway"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Unsandboxed access so Grok can run CLI commands, read/write files, and open apps on this machine on your behalf." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
				variant: online ? "success" : "warn",
				children: online ? "connected" : "offline"
			})]
		}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "space-y-3",
			children: [
				online && info ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-2.5 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldCheck, { className: "mt-0.5 h-4 w-4 shrink-0 text-[var(--color-success)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "font-medium text-[var(--color-fg)]",
								children: [
									info.user,
									"@",
									info.hostname
								]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "font-mono text-xs text-[var(--color-muted)]",
								children: [
									info.bridge,
									" · ",
									info.platform,
									"/",
									info.arch,
									" · ",
									info.homedir
								]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-1 text-xs text-[var(--color-subtle)]",
								children: [
									"Agent can use ",
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "font-mono",
										children: "$"
									}),
									" shell in chat and the Desktop host panel."
								]
							})
						]
					})]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3 text-sm text-[var(--color-muted)]",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: "No live desktop gateway. Without it, Grok cannot control your PC — only chat." }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("ol", {
						className: "list-decimal space-y-1 pl-4 text-xs text-[var(--color-subtle)]",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", { children: [
								"Install/run the Arch package:",
								" ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "font-mono text-[var(--color-fg)]",
									children: "grokhub"
								})
							] }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: "Use the Electron window (not a plain browser tab)" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: "Click Connect below to probe host IPC / API" })
						]
					})]
				}),
				error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-danger)]",
					children: error
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [
						!online ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							disabled: busy,
							onClick: () => void connectGateway(),
							children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-4 w-4 animate-spin" }), "Connecting…"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plug, { className: "h-4 w-4" }), "Connect desktop host"] })
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							onClick: () => {
								setNav("desktop");
								onOpenDesktop?.();
							},
							children: "Open Desktop host"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							variant: "secondary",
							size: "default",
							disabled: busy,
							onClick: () => void probe(),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: cn("h-4 w-4", busy && "animate-spin") }), "Reprobe"]
						}),
						isElectron && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "info",
							className: "self-center",
							children: "Electron shell"
						})
					]
				})
			]
		})]
	});
}
//#endregion
export { HostGatewayBanner as t };
