import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dgqzm_EC.mjs";
import { F as Folder, I as FolderOpen, U as ChevronRight, Y as AppWindow, d as ShieldAlert, m as RefreshCw, s as Terminal, v as Play } from "../_libs/lucide-react.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
import { t as HostGatewayBanner } from "./HostGatewayBanner-DTFXC7I1.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/DesktopHostView-DAnHL72y.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var QUICK_CMDS = [
	"uname -a && whoami && pwd",
	"ls -la",
	"df -h | head -12",
	"ps aux --sort=-%mem | head -12",
	"env | sort | head -40"
];
function DesktopHostView() {
	const recordUsage = useGrokHub((s) => s.recordUsage);
	const [api, setApi] = (0, import_react.useState)(null);
	const [tab, setTab] = (0, import_react.useState)("cli");
	const [info, setInfo] = (0, import_react.useState)(null);
	const [error, setError] = (0, import_react.useState)(null);
	const [loading, setLoading] = (0, import_react.useState)(true);
	const [cmd, setCmd] = (0, import_react.useState)("uname -a && whoami && pwd");
	const [cwd, setCwd] = (0, import_react.useState)("");
	const [busy, setBusy] = (0, import_react.useState)(false);
	const [result, setResult] = (0, import_react.useState)(null);
	const [history, setHistory] = (0, import_react.useState)([]);
	const [dirPath, setDirPath] = (0, import_react.useState)("");
	const [entries, setEntries] = (0, import_react.useState)([]);
	const [filePreview, setFilePreview] = (0, import_react.useState)(null);
	const [apps, setApps] = (0, import_react.useState)([]);
	const [appQ, setAppQ] = (0, import_react.useState)("");
	const [isShell, setIsShell] = (0, import_react.useState)(false);
	const probed = (0, import_react.useRef)(false);
	(0, import_react.useEffect)(() => {
		let cancelled = false;
		(async () => {
			try {
				const mod = await import("./host-client-D2zxhrZc.mjs");
				if (cancelled) return;
				setApi(mod);
				setIsShell(mod.isDesktopShell());
				setLoading(true);
				const i = await mod.hostInfo();
				if (cancelled) return;
				setInfo(i);
				setCwd(i.homedir || i.cwd);
				setDirPath(i.homedir || i.cwd);
				if (i.bridge === "none" || !i.unsandboxed) setError("Desktop host bridge is offline. Fully quit and relaunch GrokHub from the Arch package (Electron shell). Browser-only preview has limited host access.");
				if (!probed.current && i.bridge !== "none") {
					probed.current = true;
					try {
						const r = await mod.hostExec("uname -a && whoami && pwd && echo --- && ls -la | head -20", i.homedir || i.cwd);
						if (!cancelled) {
							setResult(r);
							setHistory(["uname -a && whoami && pwd && echo --- && ls -la | head -20"]);
							if (!r.ok) setError(r.stderr || `probe exit ${r.code}`);
						}
					} catch (e) {
						if (!cancelled) setError(e instanceof Error ? e.message : "host probe failed");
					}
				}
			} catch (e) {
				if (!cancelled) setError(e instanceof Error ? e.message : "host bridge failed");
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [recordUsage]);
	const refreshInfo = (0, import_react.useCallback)(async () => {
		if (!api) return;
		setLoading(true);
		setError(null);
		try {
			const i = await api.hostInfo();
			setInfo(i);
			setCwd((c) => c || i.cwd);
			setDirPath((p) => p || i.homedir || i.cwd);
		} catch (e) {
			setError(e instanceof Error ? e.message : "host bridge failed");
		} finally {
			setLoading(false);
		}
	}, [api]);
	async function runCmd(command) {
		if (!api) return;
		const c = (command ?? cmd).trim();
		if (!c) return;
		if (!recordUsage("host").ok) {
			setError("Subscription unit quota exceeded — reset period or switch plan in Settings.");
			return;
		}
		setBusy(true);
		setError(null);
		try {
			const r = await api.hostExec(c, cwd || void 0);
			setResult(r);
			setHistory((h) => [c, ...h.filter((x) => x !== c)].slice(0, 12));
		} catch (e) {
			setError(e instanceof Error ? e.message : "exec failed");
		} finally {
			setBusy(false);
		}
	}
	async function loadDir(p) {
		if (!api) return;
		setBusy(true);
		setError(null);
		setFilePreview(null);
		try {
			const res = await api.hostListDir(p || dirPath);
			setDirPath(res.path);
			setEntries(res.entries);
		} catch (e) {
			setError(e instanceof Error ? e.message : "list dir failed");
		} finally {
			setBusy(false);
		}
	}
	async function openEntry(e) {
		if (!api) return;
		if (e.isDir) {
			await loadDir(e.path);
			return;
		}
		setBusy(true);
		try {
			const f = await api.hostReadFile(e.path);
			setFilePreview({
				path: f.path,
				content: f.content
			});
		} catch (err) {
			setError(err instanceof Error ? err.message : "read failed");
		} finally {
			setBusy(false);
		}
	}
	async function loadApps() {
		if (!api) return;
		setBusy(true);
		setError(null);
		try {
			setApps(await api.hostListApps());
		} catch (e) {
			setError(e instanceof Error ? e.message : "list apps failed");
		} finally {
			setBusy(false);
		}
	}
	(0, import_react.useEffect)(() => {
		if (!api) return;
		if (tab === "files" && entries.length === 0 && dirPath) loadDir(dirPath);
		if (tab === "apps" && apps.length === 0) loadApps();
	}, [tab, api]);
	const filteredApps = apps.filter((a) => a.name.toLowerCase().includes(appQ.trim().toLowerCase()));
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HostGatewayBanner, { variant: "card" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
				className: "gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldAlert, { className: "h-4 w-4 text-[var(--color-warn)]" }), "Desktop host · session"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Full host CLI / files / apps. Shell commands bill 0.25 units against your plan." })]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "flex flex-wrap items-center gap-2",
				children: [
					loading && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Probing host…"
					}),
					info && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: info.unsandboxed ? "warn" : "default",
							children: info.unsandboxed ? "unsandboxed" : "limited"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "info",
							children: info.bridge
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
							className: "font-mono",
							children: [
								info.platform,
								"/",
								info.arch
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: [
								info.user,
								"@",
								info.hostname,
								" · ",
								info.shell
							]
						}),
						isShell && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "success",
							children: "Electron shell"
						})
					] }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						size: "sm",
						variant: "secondary",
						className: "ml-auto",
						onClick: () => void refreshInfo(),
						disabled: !api,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "h-3.5 w-3.5" }), "Refresh"]
					})
				]
			})] }),
			error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_10%,transparent)] px-3 py-2 text-sm text-[var(--color-danger)]",
				children: error
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex flex-wrap gap-1.5",
				children: [
					[
						"cli",
						"CLI",
						Terminal
					],
					[
						"files",
						"Files",
						FolderOpen
					],
					[
						"apps",
						"Apps",
						AppWindow
					]
				].map(([id, label, Icon]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					onClick: () => setTab(id),
					className: cn("inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border px-3 text-sm", tab === id ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : "border-[var(--color-border)] text-[var(--color-muted)]"),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-3.5 w-3.5" }), label]
				}, id))
			}),
			tab === "cli" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Shell"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Full command access · 0.25u per run" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "grid gap-2 sm:grid-cols-[1fr_160px]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: cwd,
							onChange: (e) => setCwd(e.target.value),
							placeholder: "Working directory",
							className: "font-mono text-xs"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							disabled: !api || busy,
							onClick: () => void runCmd("pwd && ls -la"),
							children: "pwd + ls"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
						className: "flex gap-2",
						onSubmit: (e) => {
							e.preventDefault();
							runCmd();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: cmd,
							onChange: (e) => setCmd(e.target.value),
							placeholder: "e.g. systemctl --user status · pacman -Q electron",
							className: "font-mono text-xs",
							disabled: busy || !api
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							type: "submit",
							disabled: busy || !api || !cmd.trim(),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "h-3.5 w-3.5" }), "Run"]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-1.5",
						children: QUICK_CMDS.map((h) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]",
							onClick: () => {
								setCmd(h);
								runCmd(h);
							},
							children: [h.slice(0, 42), h.length > 42 ? "…" : ""]
						}, h))
					}),
					result && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[#0c0c0e]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center justify-between border-b border-[var(--color-border)] px-3 py-2 text-[10px] text-[var(--color-subtle)]",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "font-mono",
								children: ["$ ", result.command]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
								"exit ",
								result.code ?? "?",
								" · ",
								result.ms,
								"ms"
							] })]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("pre", {
							className: "max-h-[360px] overflow-auto p-3 font-mono text-xs leading-relaxed text-[var(--color-fg)] whitespace-pre-wrap",
							children: [
								result.stdout || "",
								result.stderr ? `\n[stderr]\n${result.stderr}` : "",
								!result.stdout && !result.stderr ? "(no output)" : ""
							]
						})]
					})
				]
			})] }),
			tab === "files" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid gap-3 lg:grid-cols-[1fr_1fr]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
					className: "gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-sm",
						children: "Filesystem"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: dirPath,
							onChange: (e) => setDirPath(e.target.value),
							className: "font-mono text-xs",
							onKeyDown: (e) => {
								if (e.key === "Enter") loadDir(dirPath);
							}
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							size: "sm",
							variant: "secondary",
							disabled: !api,
							onClick: () => void loadDir(dirPath),
							children: "Open"
						})]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("ul", {
					className: "max-h-[420px] space-y-0.5 overflow-auto",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						className: "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
						onClick: () => {
							loadDir(dirPath.replace(/\/[^/]+\/?$/, "") || "/");
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Folder, { className: "h-3.5 w-3.5" }), ".."]
					}) }), entries.map((e) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => void openEntry(e),
						className: "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-[var(--color-elevated)]",
						children: [e.isDir ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "h-3.5 w-3.5 text-[var(--color-muted)]" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "h-3.5 w-3.5 text-[var(--color-subtle)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "min-w-0 flex-1 truncate",
							children: e.name
						})]
					}) }, e.path))]
				}) })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
					className: "text-sm",
					children: "Preview"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
					className: "font-mono text-[10px]",
					children: filePreview?.path || "Select a file"
				})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
					className: "max-h-[420px] overflow-auto rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[#0c0c0e] p-3 font-mono text-xs text-[var(--color-muted)] whitespace-pre-wrap",
					children: filePreview?.content || "—"
				}) })] })]
			}),
			tab === "apps" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
				className: "gap-2 sm:flex-row sm:items-end sm:justify-between",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
					className: "text-sm",
					children: "Installed apps (.desktop)"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Launch with gtk-launch / xdg-open as your user" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						value: appQ,
						onChange: (e) => setAppQ(e.target.value),
						placeholder: "Filter apps",
						className: "sm:w-48"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "sm",
						variant: "secondary",
						disabled: !api,
						onClick: () => void loadApps(),
						children: "Reload"
					})]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid max-h-[480px] gap-2 overflow-auto sm:grid-cols-2 xl:grid-cols-3",
				children: [filteredApps.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate text-sm font-medium",
							children: a.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate font-mono text-[10px] text-[var(--color-subtle)]",
							children: a.exec
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "sm",
						disabled: !api,
						onClick: () => void api?.hostOpenApp({
							desktopFile: a.desktopFile,
							exec: a.exec
						}),
						children: "Open"
					})]
				}, a.id)), filteredApps.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm text-[var(--color-muted)]",
					children: "No .desktop apps found yet."
				})]
			}) })] })
		]
	});
}
//#endregion
export { DesktopHostView };
