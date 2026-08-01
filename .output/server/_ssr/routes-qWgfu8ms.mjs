import { o as __toESM } from "../_runtime.mjs";
import { f as getMode, h as modeBadge, p as getModesWithCatalog, t as APP_VERSION } from "./version-RzUVPQXg.mjs";
import { F as require_react, P as require_jsx_runtime, g as require_react_dom } from "../_libs/@tanstack/react-router+[...].mjs";
import { c as useCurrentUser, l as useCurrentUserState, n as GrokHubMark, o as signOut, r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { f as useGrokHub, t as Badge } from "./card-CfSUHW0g.mjs";
import { A as History, C as MessageSquare, G as Brain, M as Hammer, R as Ellipsis, T as Menu, V as Command, W as Cable, a as Trash2, b as Minus, c as Square, f as Settings, i as Users, k as Image, l as Sparkles, n as X, o as TimerReset, t as Zap, w as MessageSquarePlus, y as Pencil } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as UsageMeterChip } from "./UsageMeter-DNjJlPqz.mjs";
import { t as ProfileAvatar } from "./ProfileAvatar-DhCtn6_K.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/routes-qWgfu8ms.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var import_react_dom = require_react_dom();
/**
* Minimal signed-in identity chip + sign-out. Restyle freely (see the
* `design-ui` skill). Sign-out is only shown when auth is enabled (the
* disabled-auth dev user has nothing to sign out of).
*/
function UserButton() {
	const user = useCurrentUser();
	if (!user) return null;
	const label = user.displayName ?? user.primaryEmail ?? "Account";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProfileAvatar, {
				src: user.profileImageUrl,
				name: user.displayName,
				email: user.primaryEmail,
				size: "sm"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "text-sm font-medium",
				children: label
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: () => void signOut(),
				className: "cursor-pointer text-sm underline-offset-4 opacity-70 hover:underline",
				children: "Sign out"
			})
		]
	});
}
var ICONS = {
	auto: Sparkles,
	fast: Zap,
	expert: Brain,
	heavy: Users,
	build: Hammer
};
/**
* Mode picker for the title bar. Menu is portaled to document.body so it is not
* clipped by the frameless shell overflow / app-region drag.
*/
function ModePicker() {
	const mode = useGrokHub((s) => s.mode);
	const open = useGrokHub((s) => s.modeMenuOpen);
	const setMode = useGrokHub((s) => s.setMode);
	const setModeMenuOpen = useGrokHub((s) => s.setModeMenuOpen);
	const catalog = useGrokHub((s) => s.modelCatalog);
	const btnRef = (0, import_react.useRef)(null);
	const menuRef = (0, import_react.useRef)(null);
	const [pos, setPos] = (0, import_react.useState)({
		top: 0,
		right: 0
	});
	const modes = getModesWithCatalog(catalog);
	const active = modes.find((m) => m.id === mode) ?? modes[0];
	const ActiveIcon = ICONS[active.id];
	const noDrag = { WebkitAppRegion: "no-drag" };
	(0, import_react.useLayoutEffect)(() => {
		if (!open || !btnRef.current) return;
		const update = () => {
			const r = btnRef.current.getBoundingClientRect();
			setPos({
				top: r.bottom + 6,
				right: Math.max(8, window.innerWidth - r.right)
			});
		};
		update();
		window.addEventListener("resize", update);
		window.addEventListener("scroll", update, true);
		return () => {
			window.removeEventListener("resize", update);
			window.removeEventListener("scroll", update, true);
		};
	}, [open]);
	(0, import_react.useEffect)(() => {
		if (!open) return;
		const onDoc = (e) => {
			const t = e.target;
			if (btnRef.current?.contains(t) || menuRef.current?.contains(t)) return;
			setModeMenuOpen(false);
		};
		const onKey = (e) => {
			if (e.key === "Escape") setModeMenuOpen(false);
		};
		document.addEventListener("mousedown", onDoc);
		document.addEventListener("keydown", onKey);
		return () => {
			document.removeEventListener("mousedown", onDoc);
			document.removeEventListener("keydown", onKey);
		};
	}, [open, setModeMenuOpen]);
	const menu = open && typeof document !== "undefined" && (0, import_react_dom.createPortal)(/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: menuRef,
		role: "listbox",
		style: {
			...noDrag,
			position: "fixed",
			top: pos.top,
			right: pos.right,
			zIndex: 9999
		},
		className: "w-[min(100vw-1.5rem,340px)] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] p-1.5 shadow-[var(--shadow-soft)]",
		children: [modes.map((m) => {
			const Icon = ICONS[m.id];
			const selected = m.id === mode;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				role: "option",
				"aria-selected": selected,
				style: noDrag,
				onClick: (e) => {
					e.preventDefault();
					e.stopPropagation();
					setMode(m.id);
					setModeMenuOpen(false);
				},
				className: cn("flex w-full items-start gap-3 rounded-[var(--radius-md)] px-2.5 py-2.5 text-left transition-colors", selected ? "bg-[var(--color-elevated)]" : "hover:bg-[var(--color-elevated)]/70"),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "mt-0.5 h-4 w-4 shrink-0 text-[var(--color-muted)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "text-sm font-medium text-[var(--color-fg)]",
									children: m.label
								}),
								m.id === "build" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "rounded bg-[var(--color-surface)] px-1 py-px text-[10px] text-[var(--color-subtle)]",
									children: "Beta"
								}),
								selected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "ml-auto text-[var(--color-muted)]",
									children: "✓"
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-xs text-[var(--color-muted)]",
							children: m.subtitle
						}),
						m.id !== "auto" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-0.5 font-mono text-[10px] text-[var(--color-subtle)]",
							children: m.modelId
						})
					]
				})]
			}, m.id);
		}), catalog.essential.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "border-t border-[var(--color-border)] px-2.5 py-1.5 text-[10px] text-[var(--color-subtle)]",
			children: [
				catalog.source === "live" ? "Live" : "Fallback",
				" · ",
				catalog.essential.length,
				" ",
				"models · slots by ",
				catalog.classifiedBy === "grok" ? "Grok" : "heuristic"
			]
		})]
	}), document.body);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative",
		style: noDrag,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			ref: btnRef,
			type: "button",
			style: noDrag,
			onClick: (e) => {
				e.preventDefault();
				e.stopPropagation();
				setModeMenuOpen(!open);
			},
			className: cn("flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 text-left transition-colors hover:border-[var(--color-border-strong)]", open && "border-[var(--color-border-strong)]"),
			"aria-haspopup": "listbox",
			"aria-expanded": open,
			"aria-label": `Model mode: ${modeBadge(active.id, catalog)}`,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActiveIcon, { className: "h-3.5 w-3.5 text-[var(--color-muted)]" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-xs font-medium text-[var(--color-fg)]",
					children: active.label
				}),
				active.id === "build" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "rounded bg-[var(--color-surface)] px-1 py-px text-[10px] text-[var(--color-subtle)]",
					children: "Beta"
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "hidden max-w-[7.5rem] truncate font-mono text-[10px] text-[var(--color-subtle)] sm:inline",
					children: active.model
				})
			]
		}), menu]
	});
}
var AgentsView = (0, import_react.lazy)(() => import("./AgentsView-Cf6njxq0.mjs").then((m) => ({ default: m.AgentsView })));
var AutomationsView = (0, import_react.lazy)(() => import("./AutomationsView-yWK1NnFI.mjs").then((m) => ({ default: m.AutomationsView })));
var ChatView = (0, import_react.lazy)(() => import("./ChatView-BJWfY-J4.mjs").then((m) => ({ default: m.ChatView })));
var CommandView = (0, import_react.lazy)(() => import("./CommandView-c4h5pUng.mjs").then((m) => ({ default: m.CommandView })));
var ConnectorsView = (0, import_react.lazy)(() => import("./ConnectorsView-Bc6wKqp4.mjs").then((m) => ({ default: m.ConnectorsView })));
var DesktopHostView = (0, import_react.lazy)(() => import("./DesktopHostView-CIKco7XU.mjs").then((m) => ({ default: m.DesktopHostView })));
var HistoryView = (0, import_react.lazy)(() => import("./HistoryView-rH8eyjeR.mjs").then((m) => ({ default: m.HistoryView })));
var ImagineView = (0, import_react.lazy)(() => import("./ImagineView-DU3GYr6t.mjs").then((m) => ({ default: m.ImagineView })));
var SettingsView = (0, import_react.lazy)(() => import("./SettingsView-GcZ7ZN2w.mjs").then((m) => ({ default: m.SettingsView })));
var SkillsView = (0, import_react.lazy)(() => import("./SkillsView-DEUET5hZ.mjs").then((m) => ({ default: m.SkillsView })));
var NAV = [
	{
		id: "chat",
		label: "Agent",
		icon: MessageSquare
	},
	{
		id: "history",
		label: "History",
		icon: History
	},
	{
		id: "command",
		label: "Command",
		icon: Command
	},
	{
		id: "imagine",
		label: "Imagine",
		icon: Image
	},
	{
		id: "connectors",
		label: "Connectors",
		icon: Cable
	},
	{
		id: "skills",
		label: "Skills",
		icon: Sparkles
	},
	{
		id: "automations",
		label: "Automations",
		icon: TimerReset
	},
	{
		id: "agents",
		label: "Roster",
		icon: Users
	},
	{
		id: "settings",
		label: "Settings",
		icon: Settings
	}
];
function RecentThreadRow({ id, title, active, onSelect }) {
	const renameThread = useGrokHub((s) => s.renameThread);
	const deleteThread = useGrokHub((s) => s.deleteThread);
	const [menuOpen, setMenuOpen] = (0, import_react.useState)(false);
	const [renaming, setRenaming] = (0, import_react.useState)(false);
	const [draft, setDraft] = (0, import_react.useState)(title);
	const menuRef = (0, import_react.useRef)(null);
	const inputRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		setDraft(title);
	}, [title]);
	(0, import_react.useEffect)(() => {
		if (!menuOpen) return;
		const onDoc = (e) => {
			if (!menuRef.current?.contains(e.target)) setMenuOpen(false);
		};
		document.addEventListener("mousedown", onDoc);
		return () => document.removeEventListener("mousedown", onDoc);
	}, [menuOpen]);
	(0, import_react.useEffect)(() => {
		if (renaming) {
			inputRef.current?.focus();
			inputRef.current?.select();
		}
	}, [renaming]);
	function commitRename() {
		const next = draft.trim();
		if (next && next !== title) renameThread(id, next);
		else setDraft(title);
		setRenaming(false);
	}
	if (renaming) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "mb-0.5 px-1 py-0.5",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
			ref: inputRef,
			value: draft,
			onChange: (e) => setDraft(e.target.value),
			onBlur: commitRename,
			onKeyDown: (e) => {
				if (e.key === "Enter") {
					e.preventDefault();
					commitRename();
				}
				if (e.key === "Escape") {
					setDraft(title);
					setRenaming(false);
				}
			},
			className: "w-full rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-2 py-1 text-xs text-[var(--color-fg)] outline-none focus:ring-1 focus:ring-[var(--color-accent)]",
			"aria-label": "Rename chat"
		})
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("group relative mb-0.5 flex w-full items-center gap-0.5 rounded-[var(--radius-sm)]", active ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/50"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			onClick: onSelect,
			className: "min-w-0 flex-1 truncate px-2.5 py-1.5 text-left text-xs font-medium",
			children: title
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "relative shrink-0 pr-0.5",
			ref: menuRef,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: cn("rounded p-1 text-[var(--color-subtle)] transition-opacity hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]", menuOpen ? "opacity-100" : "opacity-0 group-hover:opacity-100 focus:opacity-100"),
				"aria-label": "Chat options",
				onClick: (e) => {
					e.stopPropagation();
					setMenuOpen((v) => !v);
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Ellipsis, { className: "h-3.5 w-3.5" })
			}), menuOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "absolute right-0 top-full z-50 mt-0.5 min-w-[132px] overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] py-1 shadow-lg",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: "flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-[var(--color-fg)] hover:bg-[var(--color-surface)]",
					onClick: () => {
						setMenuOpen(false);
						setDraft(title);
						setRenaming(true);
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "h-3 w-3" }), "Rename"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: "flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-[var(--color-danger)] hover:bg-[var(--color-surface)]",
					onClick: () => {
						setMenuOpen(false);
						deleteThread(id);
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "h-3 w-3" }), "Delete"]
				})]
			})]
		})]
	});
}
function AppShell() {
	const nav = useGrokHub((s) => s.nav);
	const setNav = useGrokHub((s) => s.setNav);
	const heartbeatAt = useGrokHub((s) => s.heartbeatAt);
	const running = useGrokHub((s) => s.running);
	const mode = useGrokHub((s) => s.mode);
	const tickHeartbeat = useGrokHub((s) => s.tickHeartbeat);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
	const probeGrok = useGrokHub((s) => s.probeGrok);
	const syncFromGrok = useGrokHub((s) => s.syncFromGrok);
	const newThread = useGrokHub((s) => s.newThread);
	const threads = useGrokHub((s) => s.threads);
	const selectThread = useGrokHub((s) => s.selectThread);
	const activeThreadId = useGrokHub((s) => s.activeThreadId);
	const oauth = useGrokHub((s) => s.oauth);
	const profile = useGrokHub((s) => s.profile);
	const { user, isPending } = useCurrentUserState();
	const [mobileOpen, setMobileOpen] = (0, import_react.useState)(false);
	const [isDesktop, setIsDesktop] = (0, import_react.useState)(false);
	const modeMeta = getMode(mode);
	const accountLabel = oauth?.name || oauth?.email || profile?.displayName || profile?.email || (user && !user.isDevFallback ? user.displayName || user.primaryEmail || null : null);
	const accountConnected = Boolean(oauth?.accessToken || user && !user.isDevFallback || grokConnected);
	(0, import_react.useEffect)(() => {
		const p = useGrokHub.persist.rehydrate();
		Promise.resolve(p).finally(() => {
			const cur = useGrokHub.getState().nav;
			useGrokHub.setState({ nav: cur === "desktop" ? "chat" : "chat" });
			const st = useGrokHub.getState();
			st.refreshStaleTimes();
			st.tickHeartbeat();
			st.hydrateSecrets().then(() => {
				useGrokHub.getState().probeGrok();
				useGrokHub.getState().refreshUsage();
			});
			if (st.oauth?.accessToken) useGrokHub.setState({ connectors: st.connectors.map((c) => c.id === "grok-xai" ? {
				...c,
				status: "connected",
				lastUsed: Date.now()
			} : c) });
			useGrokHub.getState().probeGrok();
			useGrokHub.getState().refreshModels();
			(async () => {
				try {
					const { hostInfo } = await import("./host-client-D2zxhrZc.mjs");
					const info = await hostInfo();
					if (info.unsandboxed && info.bridge !== "none") useGrokHub.setState((s) => ({ connectors: s.connectors.map((c) => c.id === "desktop-host" ? {
						...c,
						status: "connected",
						lastUsed: Date.now()
					} : c) }));
				} catch {}
			})();
		});
		setIsDesktop(Boolean(window.grokhubDesktop));
		const flush = () => {
			try {
				useGrokHub.persist.rehydrate;
				useGrokHub.setState((s) => ({ heartbeatAt: s.heartbeatAt }));
			} catch {}
		};
		window.addEventListener("beforeunload", flush);
		document.addEventListener("visibilitychange", () => {
			if (document.visibilityState === "hidden") flush();
		});
		return () => {
			window.removeEventListener("beforeunload", flush);
		};
	}, []);
	(0, import_react.useEffect)(() => {
		const MODELS_POLL_MS = 300 * 1e3;
		const tick = () => {
			const st = useGrokHub.getState();
			if (st.oauth?.accessToken || st.apiKey || st.grokConnected) st.refreshModels();
		};
		const id = window.setInterval(tick, MODELS_POLL_MS);
		const t = window.setTimeout(tick, 8e3);
		return () => {
			window.clearInterval(id);
			window.clearTimeout(t);
		};
	}, []);
	(0, import_react.useEffect)(() => {
		const USAGE_POLL_MS = 6e4;
		const tick = () => {
			const st = useGrokHub.getState();
			if (st.ssoCookie || st.oauth?.accessToken || st.apiKey || document.visibilityState === "visible") st.refreshUsage();
			if (st.ssoCookie) st.syncWebsiteConnectors();
		};
		tick();
		const id = window.setInterval(tick, USAGE_POLL_MS);
		const onVis = () => {
			if (document.visibilityState === "visible") tick();
		};
		document.addEventListener("visibilitychange", onVis);
		return () => {
			window.clearInterval(id);
			document.removeEventListener("visibilitychange", onVis);
		};
	}, []);
	(0, import_react.useEffect)(() => {
		const id = window.setInterval(() => {
			useGrokHub.getState().tickAutomations();
		}, 3e4);
		const t = window.setTimeout(() => void useGrokHub.getState().tickAutomations(), 5e3);
		return () => {
			window.clearInterval(id);
			window.clearTimeout(t);
		};
	}, []);
	(0, import_react.useEffect)(() => {
		if (oauth?.name || oauth?.email) {
			syncFromGrok({
				displayName: oauth.name ?? null,
				email: oauth.email ?? null,
				imageUrl: oauth.picture ?? null
			});
			return;
		}
		if (isPending) return;
		if (user && !user.isDevFallback) syncFromGrok({
			displayName: user.displayName,
			email: user.primaryEmail,
			imageUrl: user.profileImageUrl
		});
	}, [
		user,
		isPending,
		syncFromGrok,
		oauth?.name,
		oauth?.email,
		oauth?.picture
	]);
	(0, import_react.useEffect)(() => {
		const hb = window.setInterval(() => tickHeartbeat(), 3e4);
		return () => window.clearInterval(hb);
	}, [tickHeartbeat]);
	(0, import_react.useEffect)(() => {
		setMobileOpen(false);
	}, [nav]);
	const drag = { WebkitAppRegion: "drag" };
	const noDrag = { WebkitAppRegion: "no-drag" };
	const recent = [...threads].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 6);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-dvh max-h-dvh w-full max-w-none flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex h-10 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-3",
			style: drag,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2",
				style: noDrag,
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GrokHubMark, { className: "h-6 w-6" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-xs font-semibold tracking-tight",
						children: "GrokHub"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
						className: "hidden font-mono text-[10px] sm:inline-flex",
						children: ["v", APP_VERSION]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => void probeGrok(),
						className: "hidden items-center gap-1.5 rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] md:inline-flex",
						title: grokStatusDetail,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("inline-block h-1.5 w-1.5 rounded-full", grokConnected === true ? "bg-[var(--color-success)]" : grokConnected === false ? "bg-[var(--color-danger)]" : "bg-[var(--color-subtle)]") }), grokConnected === true ? "Grok live" : grokConnected === false ? "Grok offline" : "Grok…"]
					})
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-2",
				style: noDrag,
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "hidden max-w-[160px] sm:flex" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ModePicker, {}),
					isPending && !oauth ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "hidden h-7 w-20 animate-pulse rounded bg-[var(--color-elevated)] sm:block" }) : accountLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: () => setNav("settings"),
						className: "hidden max-w-[10rem] truncate rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)] sm:inline",
						title: accountLabel,
						children: accountLabel
					}) : user && !user.isDevFallback ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "hidden scale-90 sm:block",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UserButton, {})
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: () => setNav("settings"),
						className: "hidden rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)] sm:inline",
						children: "Connect Grok"
					}),
					isDesktop && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "ml-1 flex items-center gap-0.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
								onClick: () => window.grokhubDesktop?.minimize?.(),
								"aria-label": "Minimize",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, { className: "h-3.5 w-3.5" })
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
								onClick: () => window.grokhubDesktop?.maximize?.(),
								"aria-label": "Maximize",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Square, { className: "h-3 w-3" })
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[color-mix(in_oklab,var(--color-danger)_25%,transparent)] hover:text-[var(--color-danger)]",
								onClick: () => window.grokhubDesktop?.close?.(),
								"aria-label": "Close",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "h-3.5 w-3.5" })
							})
						]
					})
				]
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "app-frame flex min-h-0 w-full flex-1 overflow-hidden",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", {
				className: "sidebar-rail hidden shrink-0 flex-col overflow-hidden border-r border-[var(--color-border)] bg-[var(--color-surface)] md:flex",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "shrink-0 p-3 pb-1",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							size: "sm",
							className: "w-full",
							variant: "secondary",
							onClick: () => newThread(),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquarePlus, { className: "h-4 w-4" }), "New chat"]
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("nav", {
						className: "scroll-panel flex flex-1 flex-col gap-1 p-3 pt-1",
						children: [NAV.map((item) => {
							const Icon = item.icon;
							return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
								type: "button",
								onClick: () => setNav(item.id),
								className: cn("flex h-10 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors", nav === item.id ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]"),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-4 w-4 shrink-0" }), item.label]
							}, item.id);
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-3 border-t border-[var(--color-border)] pt-3",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "mb-1.5 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]",
									children: "Recent"
								}),
								recent.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RecentThreadRow, {
									id: t.id,
									title: t.title,
									active: t.id === activeThreadId,
									onSelect: () => selectThread(t.id)
								}, t.id)),
								recent.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
									className: "px-2 py-1 text-[11px] text-[var(--color-subtle)]",
									children: "No chats yet"
								})
							]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "shrink-0 space-y-2 border-t border-[var(--color-border)] p-4",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2 text-xs text-[var(--color-muted)]",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "pulse-live inline-block h-2 w-2 rounded-full bg-[var(--color-success)]" }),
								"Heartbeat ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: heartbeatAt })
							]
						})
					})
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "app-stage flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("header", {
						className: "flex shrink-0 items-center justify-between gap-3 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-bg)_88%,transparent)] px-4 py-3 backdrop-blur-md md:px-6 3xl:px-8 uw:px-10",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 items-center gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon",
								className: "md:hidden",
								onClick: () => setMobileOpen((v) => !v),
								"aria-label": "Menu",
								children: mobileOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "h-5 w-5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Menu, { className: "h-5 w-5" })
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium md:text-base",
									children: nav === "desktop" ? "Desktop host" : NAV.find((n) => n.id === nav)?.label ?? "Agent"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "truncate text-xs text-[var(--color-subtle)]",
									children: [
										"GrokHub v",
										APP_VERSION,
										" ·",
										" ",
										accountLabel ? accountLabel : accountConnected ? grokStatusDetail || "Grok connected" : "Connect Grok in Settings"
									]
								})]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex shrink-0 items-center gap-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "max-w-[140px] sm:hidden" }),
								running ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: "info",
									children: "Working"
								}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: "success",
									children: "Online"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
									className: "hidden font-mono sm:inline-flex",
									children: [
										modeMeta.label,
										" · ",
										modeMeta.model
									]
								})
							]
						})]
					}),
					mobileOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "shrink-0 border-b border-[var(--color-border)] bg-[var(--color-surface)] p-2 md:hidden",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mb-2 px-1",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "w-full" })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "grid grid-cols-2 gap-1",
							children: NAV.map((item) => {
								const Icon = item.icon;
								return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									type: "button",
									onClick: () => setNav(item.id),
									className: cn("flex h-11 items-center gap-2 rounded-[var(--radius-sm)] px-3 text-sm", nav === item.id ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)]"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-4 w-4" }), item.label]
								}, item.id);
							})
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("main", {
						className: "app-stage flex min-h-0 flex-1 flex-col overflow-hidden p-3 sm:p-4 md:p-5 3xl:p-6 uw:p-8",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_react.Suspense, {
							fallback: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "flex flex-1 items-center justify-center text-sm text-[var(--color-subtle)]",
								children: "Loading…"
							}),
							children: (nav === "chat" || nav === "history") && nav === "chat" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "chat-stage min-h-0 flex-1 overflow-hidden",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChatView, {})
							}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "scroll-panel min-h-0 flex-1",
								children: [
									nav === "history" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HistoryView, {}),
									nav === "command" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandView, {}),
									nav === "connectors" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ConnectorsView, {}),
									nav === "skills" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SkillsView, {}),
									nav === "automations" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AutomationsView, {}),
									nav === "agents" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentsView, {}),
									nav === "imagine" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImagineView, {}),
									nav === "desktop" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DesktopHostView, {}),
									nav === "settings" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsView, {})
								]
							})
						})
					})
				]
			})]
		})]
	});
}
function HomePage() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppShell, {});
}
//#endregion
export { HomePage as component };
