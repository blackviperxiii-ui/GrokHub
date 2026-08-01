import { P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, i as CardDescription, n as Card, o as CardTitle, r as CardContent, s as useGrokHub, t as Badge } from "./card-J2wZc2w_.mjs";
import { F as Download, S as LoaderCircle, T as Image, c as Sparkles } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/ImagineView-CiX7X4e6.js
var import_jsx_runtime = require_jsx_runtime();
var ASPECTS = [
	"1:1",
	"16:9",
	"9:16",
	"3:2",
	"2:3"
];
function ImagineView() {
	const prompt = useGrokHub((s) => s.imaginePrompt);
	const aspect = useGrokHub((s) => s.imagineAspect);
	const jobs = useGrokHub((s) => s.imagineJobs);
	const busy = useGrokHub((s) => s.imagineBusy);
	const err = useGrokHub((s) => s.imagineError);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const mode = useGrokHub((s) => s.mode);
	const setImaginePrompt = useGrokHub((s) => s.setImaginePrompt);
	const setImagineAspect = useGrokHub((s) => s.setImagineAspect);
	const runImagine = useGrokHub((s) => s.runImagine);
	const latest = jobs[0];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Image, { className: "h-4 w-4" }), "Imagine"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Live Grok image generation when OAuth/API is connected; local SVG preview as fallback." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [
							ASPECTS.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => setImagineAspect(a),
								className: a === aspect ? "rounded-full border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-3 py-1.5 text-xs font-medium" : "rounded-full border border-[var(--color-border)] px-3 py-1.5 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
								children: a
							}, a)),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								className: "ml-auto font-mono",
								children: mode
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: grokConnected ? "success" : "default",
								children: grokConnected ? "live ready" : "local only"
							})
						]
					}),
					err && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-warn)]",
						children: [
							"Live Imagine: ",
							err,
							" — showing local preview."
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
						className: "flex flex-col gap-2 sm:flex-row",
						onSubmit: (e) => {
							e.preventDefault();
							runImagine();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: prompt,
							onChange: (e) => setImaginePrompt(e.target.value),
							placeholder: "Moody night desk, dual monitors, soft amber lamp, film still…",
							disabled: busy
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "submit",
							disabled: busy || !prompt.trim(),
							className: "sm:w-36",
							children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-4 w-4 animate-spin" }), "Render"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-4 w-4" }), "Generate"] })
						})]
					})
				]
			})] }),
			latest?.imageDataUrl && latest.status === "ready" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "overflow-hidden",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
					className: "flex-row items-center justify-between gap-3 space-y-0",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-sm",
						children: "Latest"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
						className: "line-clamp-1",
						children: latest.prompt
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
						href: latest.imageDataUrl,
						download: `grokhub-imagine-${latest.id}.${latest.imageDataUrl.startsWith("data:image/svg") ? "svg" : "png"}`,
						className: "inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, { className: "h-3.5 w-3.5" }), "Save"]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)]",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
						src: latest.imageDataUrl,
						alt: latest.prompt,
						className: "mx-auto max-h-[min(70vh,640px)] w-full object-contain"
					})
				}) })]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-3 sm:grid-cols-2 xl:grid-cols-3",
				children: jobs.map((job) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "overflow-hidden",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "aspect-video bg-[var(--color-surface)]",
						children: job.imageDataUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
							src: job.imageDataUrl,
							alt: job.prompt,
							className: "h-full w-full object-cover"
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex h-full items-center justify-center text-xs text-[var(--color-subtle)]",
							children: job.status === "rendering" ? "Rendering…" : "Queued"
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "space-y-1 p-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center justify-between gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: job.status === "ready" ? "success" : "info",
									children: job.status
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, {
									ts: job.ts,
									className: "text-[10px] text-[var(--color-subtle)]"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "line-clamp-2 text-xs text-[var(--color-muted)]",
								children: job.prompt
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "font-mono text-[10px] text-[var(--color-subtle)]",
								children: [
									job.aspect,
									" · ",
									job.mode
								]
							})
						]
					})]
				}, job.id))
			}),
			jobs.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "py-12 text-center text-sm text-[var(--color-muted)]",
				children: "No renders yet. Describe a scene and hit Generate."
			}) })
		]
	});
}
//#endregion
export { ImagineView };
