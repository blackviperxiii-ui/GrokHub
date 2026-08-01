import { o as __toESM } from "../_runtime.mjs";
import { C as usagePercent, n as PLAN_LIMITS, u as formatUnits, w as usageTone } from "./version-DpPTbU9E.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dx8p0izU.mjs";
import { D as Link2, N as Gauge, m as RefreshCw } from "../_libs/lucide-react.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
import { n as formatUsdFromCents, t as formatResetAt } from "./grok-website-usage-CUFrqYHh.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/UsageMeter-CiJyKV3G.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function barColor(tone) {
	if (tone === "danger") return "bg-[var(--color-danger)]";
	if (tone === "warn") return "bg-[var(--color-warn)]";
	return "bg-[var(--color-info)]";
}
var PRODUCT_COLORS = [
	"bg-[var(--color-info)]",
	"bg-[var(--color-success)]",
	"bg-[var(--color-warn)]",
	"bg-[var(--color-danger)]",
	"bg-[var(--color-muted)]"
];
/** Compact titlebar chip — shows website weekly % when available */
function UsageMeterChip({ className }) {
	const usage = useGrokHub((s) => s.usage);
	const setNav = useGrokHub((s) => s.setNav);
	const web = usage.website;
	const pct = web?.creditUsagePercent != null && usage.source === "website" ? web.creditUsagePercent : usagePercent(usage);
	const tone = usageTone(pct);
	const label = web?.planLabel || PLAN_LIMITS[usage.plan].label;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		style: { WebkitAppRegion: "no-drag" },
		onClick: (e) => {
			e.preventDefault();
			e.stopPropagation();
			setNav("settings");
		},
		title: web ? `${label}: ${Math.round(pct)}% weekly · resets ${formatResetAt(web.periodEnd)}` : `${label}: ${formatUnits(usage.usedUnits)} units · open Settings`,
		className: cn("flex min-w-0 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1 text-left transition-colors hover:border-[var(--color-border-strong)]", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Gauge, { className: cn("h-3.5 w-3.5 shrink-0", tone === "danger" ? "text-[var(--color-danger)]" : tone === "warn" ? "text-[var(--color-warn)]" : "text-[var(--color-muted)]") }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-0 flex-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate text-[10px] font-medium text-[var(--color-fg)]",
					children: label
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "tabular text-[10px] text-[var(--color-subtle)]",
					children: [Math.round(pct), "%"]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-0.5 h-1 overflow-hidden rounded-full bg-[var(--color-border)]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: cn("h-full rounded-full transition-all duration-300", barColor(tone)),
					style: { width: `${Math.min(100, Math.max(0, pct))}%` }
				})
			})]
		})]
	});
}
/** Full panel matching Grok website Settings → Usage */
function UsageMeterPanel({ compact }) {
	const usage = useGrokHub((s) => s.usage);
	const ssoCookie = useGrokHub((s) => s.ssoCookie);
	const setSsoCookie = useGrokHub((s) => s.setSsoCookie);
	const linkWebsite = useGrokHub((s) => s.linkGrokWebsiteSession);
	const refreshUsage = useGrokHub((s) => s.refreshUsage);
	const [ssoDraft, setSsoDraft] = (0, import_react.useState)("");
	const [linkBusy, setLinkBusy] = (0, import_react.useState)(false);
	const [linkDetail, setLinkDetail] = (0, import_react.useState)(null);
	const web = usage.website;
	const pct = web?.creditUsagePercent ?? usagePercent(usage);
	const tone = usageTone(pct);
	const planTitle = web?.periodType === "weekly" ? `Weekly ${web.planLabel || "SuperGrok"} Limit` : web?.planLabel ? `${web.planLabel} usage` : "Subscription usage";
	const products = (web?.productUsage || []).filter((p) => p.usagePercent > 0);
	async function onLink() {
		setLinkBusy(true);
		setLinkDetail(null);
		const r = await linkWebsite();
		setLinkDetail(r.detail);
		setLinkBusy(false);
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, {
		className: compact ? "pb-2" : void 0,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap items-start justify-between gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Gauge, { className: "h-4 w-4 text-[var(--color-muted)]" }), "Usage"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Live from Grok website Settings → Usage · polled every minute" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
					variant: usage.source === "website" ? "success" : tone === "danger" ? "danger" : "default",
					children: usage.source === "website" ? "grok.com" : usage.source
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					variant: "secondary",
					onClick: () => void refreshUsage(),
					title: "Refresh now",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "h-3.5 w-3.5" })
				})]
			})]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
		className: "space-y-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mb-1 text-xs font-medium text-[var(--color-muted)]",
						children: planTitle
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mb-2 flex flex-wrap items-end justify-between gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "text-2xl font-semibold tabular tracking-tight",
							children: [
								Math.round(pct),
								"%",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "ml-1.5 text-sm font-normal text-[var(--color-muted)]",
									children: "used"
								})
							]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "text-xs text-[var(--color-subtle)]",
							children: ["Resets ", formatResetAt(web?.periodEnd || usage.periodEnd)]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mb-2 flex h-2.5 overflow-hidden rounded-full bg-[var(--color-border)]",
						children: products.length > 0 ? products.map((p, i) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: cn("h-full", PRODUCT_COLORS[i % PRODUCT_COLORS.length]),
							style: { width: `${Math.min(100, p.usagePercent)}%` },
							title: `${p.label}: ${Math.round(p.usagePercent)}%`
						}, p.product + i)) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: cn("h-full rounded-full transition-all", barColor(tone)),
							style: { width: `${Math.min(100, Math.max(0, pct))}%` }
						})
					}),
					products.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-[var(--color-muted)]",
						children: products.map((p, i) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "inline-flex items-center gap-1.5",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("inline-block h-1.5 w-1.5 rounded-full", PRODUCT_COLORS[i % PRODUCT_COLORS.length]) }),
								p.label,
								" ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "tabular text-[var(--color-fg)]",
									children: [Math.round(p.usagePercent), "%"]
								})
							]
						}, p.product + i))
					}),
					web?.error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-2 text-xs text-[var(--color-warn)]",
						children: web.error
					}),
					!web && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-2 text-xs text-[var(--color-subtle)]",
						children: "Link your Grok website session below to show the same weekly limit as grok.com (Build / App Builder / Chat breakdown)."
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mb-2 text-xs font-medium text-[var(--color-muted)]",
					children: "Extra Usage Credits"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex items-center justify-between gap-3",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-lg font-semibold tabular",
						children: formatUsdFromCents(web?.prepaidBalanceCents ?? 0)
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Additional Credits"
					})] })
				})]
			}),
			!compact && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-2 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-xs font-medium text-[var(--color-fg)]",
						children: "Grok website session"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "text-[11px] text-[var(--color-subtle)]",
						children: [
							"Opens an in-app Grok window (not a blank tab). Sign in until the chat home loads — the window closes when the session cookie is captured. Or paste the",
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "font-mono",
								children: "sso"
							}),
							" cookie from your browser."
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							size: "sm",
							onClick: () => void onLink(),
							disabled: linkBusy,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Link2, { className: "h-3.5 w-3.5" }), linkBusy ? "Linking…" : "Link Grok website"]
						}), ssoCookie ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "success",
							children: "SSO linked"
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: "Not linked" })]
					}),
					linkDetail && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-[11px] text-[var(--color-muted)]",
						children: linkDetail
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: ssoDraft,
							onChange: (e) => setSsoDraft(e.target.value),
							placeholder: "Or paste sso=… cookie from grok.com",
							className: "font-mono text-xs"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							size: "sm",
							variant: "secondary",
							disabled: !ssoDraft.trim(),
							onClick: () => {
								setSsoCookie(ssoDraft.trim());
								setSsoDraft("");
								setLinkDetail("SSO cookie saved — refreshing usage…");
							},
							children: "Save"
						})]
					})
				]
			}),
			usage.lastPolledAt > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "text-[10px] text-[var(--color-subtle)]",
				children: [
					"Last poll ",
					new Date(usage.lastPolledAt).toLocaleTimeString(),
					usage.source === "website" ? " · source grok.com" : ""
				]
			})
		]
	})] });
}
//#endregion
export { UsageMeterPanel as n, UsageMeterChip as t };
