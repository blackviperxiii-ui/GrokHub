import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, s as IMAGINE_PRESETS, t as Badge } from "./card-Dgqzm_EC.mjs";
import { B as Download, E as LoaderCircle, S as MicOff, _ as Plus, h as Ratio, k as Image, l as Sparkles, n as X, q as ArrowUp, r as Video, x as Mic } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as Textarea } from "./textarea-aSaPOSKs.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/ImagineView-lrIXdlXq.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var ASPECTS = [
	{
		id: "auto",
		label: "Auto"
	},
	{
		id: "1:1",
		label: "1:1"
	},
	{
		id: "3:2",
		label: "3:2"
	},
	{
		id: "2:3",
		label: "2:3"
	},
	{
		id: "16:9",
		label: "16:9"
	},
	{
		id: "9:16",
		label: "9:16"
	},
	{
		id: "4:3",
		label: "4:3"
	}
];
function Pill({ active, onClick, children, disabled, title }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
		type: "button",
		title,
		disabled,
		onClick,
		className: cn("inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full border px-3 text-xs font-medium transition-colors", active ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]", disabled && "opacity-50"),
		children
	});
}
function ImagineView() {
	const prompt = useGrokHub((s) => s.imaginePrompt ?? "");
	const aspect = useGrokHub((s) => s.imagineAspect ?? "auto");
	const mediaKind = useGrokHub((s) => s.imagineMediaKind ?? "image");
	const quality = useGrokHub((s) => s.imagineQuality ?? "speed");
	const reference = useGrokHub((s) => s.imagineReference ?? null);
	const jobs = useGrokHub((s) => s.imagineJobs ?? []);
	const busy = useGrokHub((s) => Boolean(s.imagineBusy));
	const err = useGrokHub((s) => s.imagineError);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const setImaginePrompt = useGrokHub((s) => s.setImaginePrompt);
	const setImagineAspect = useGrokHub((s) => s.setImagineAspect);
	const setImagineMediaKind = useGrokHub((s) => s.setImagineMediaKind);
	const setImagineQuality = useGrokHub((s) => s.setImagineQuality);
	const setImagineReference = useGrokHub((s) => s.setImagineReference);
	const runImagine = useGrokHub((s) => s.runImagine);
	const [listening, setListening] = (0, import_react.useState)(false);
	const [aspectOpen, setAspectOpen] = (0, import_react.useState)(false);
	const fileRef = (0, import_react.useRef)(null);
	const taRef = (0, import_react.useRef)(null);
	const latest = jobs[0];
	(0, import_react.useEffect)(() => {
		return () => {
			try {
				window.webkitSpeechRecognition || window.SpeechRecognition;
			} catch {}
		};
	}, []);
	function applyPreset(prefix) {
		const body = prompt.trim();
		if (body.toLowerCase().startsWith(prefix.toLowerCase())) return;
		setImaginePrompt(prefix + (body || ""));
		taRef.current?.focus();
	}
	function onPickFile(file) {
		if (!file) return;
		if (!file.type.startsWith("image/")) return;
		const reader = new FileReader();
		reader.onload = () => {
			const url = String(reader.result || "");
			if (url.startsWith("data:image")) setImagineReference(url);
		};
		reader.readAsDataURL(file);
	}
	function toggleMic() {
		const W = window;
		const Ctor = W.SpeechRecognition || W.webkitSpeechRecognition;
		if (!Ctor) {
			setImaginePrompt((prompt ? prompt + " " : "") + "(Voice input not supported in this environment)");
			return;
		}
		if (listening) {
			setListening(false);
			return;
		}
		const rec = new Ctor();
		rec.lang = "en-US";
		rec.interimResults = true;
		rec.continuous = false;
		rec.onresult = (ev) => {
			let text = "";
			for (let i = ev.resultIndex; i < ev.results.length; i++) text += ev.results[i][0].transcript;
			if (text.trim()) setImaginePrompt((prompt ? prompt.replace(/\s+$/, "") + " " : "") + text.trim());
		};
		rec.onerror = () => setListening(false);
		rec.onend = () => setListening(false);
		setListening(true);
		rec.start();
	}
	async function onSubmit() {
		if (busy || !prompt.trim()) return;
		await runImagine();
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-full min-h-0 flex-col gap-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center justify-between gap-2 px-0.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("h2", {
					className: "flex items-center gap-2 text-sm font-semibold",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Image, { className: "h-4 w-4" }), "Imagine"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-[var(--color-muted)]",
					children: "Website-style composer · image & video · speed / quality · aspect · reference"
				})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						variant: grokConnected ? "success" : "default",
						children: grokConnected ? "Grok live" : "Local preview"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						className: "font-mono capitalize",
						children: mediaKind
					})]
				})]
			}),
			err && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-warn)]",
				children: err
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "scroll-panel min-h-0 flex-1 space-y-4",
				children: [
					latest && (latest.imageDataUrl || latest.videoDataUrl) && latest.status === "ready" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
						className: "overflow-hidden",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
							className: "flex-row items-center justify-between gap-3 space-y-0",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
									className: "text-sm",
									children: "Latest"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
									className: "line-clamp-1",
									children: latest.prompt
								})]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex shrink-0 gap-2",
								children: [latest.imageDataUrl && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
									size: "sm",
									variant: "secondary",
									onClick: () => setImagineReference(latest.imageDataUrl || null),
									children: "Use as ref"
								}), (latest.videoDataUrl || latest.imageDataUrl) && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
									href: latest.videoDataUrl || latest.imageDataUrl,
									download: `grokhub-imagine-${latest.id}.${latest.videoDataUrl ? "mp4" : latest.imageDataUrl?.startsWith("data:image/svg") ? "svg" : "png"}`,
									className: "inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, { className: "h-3.5 w-3.5" }), "Save"]
								})]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)]",
							children: latest.videoDataUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("video", {
								src: latest.videoDataUrl,
								controls: true,
								className: "mx-auto max-h-[min(70vh,640px)] w-full bg-black"
							}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
								src: latest.imageDataUrl,
								alt: latest.prompt,
								className: "mx-auto max-h-[min(70vh,640px)] w-full object-contain"
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "mt-2 font-mono text-[10px] text-[var(--color-subtle)]",
							children: [
								latest.aspect,
								" · ",
								latest.quality || "speed",
								" · ",
								latest.mediaKind || "image",
								latest.model ? ` · ${latest.model}` : "",
								latest.source ? ` · ${latest.source}` : ""
							]
						})] })]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-1.5",
						children: IMAGINE_PRESETS.map((pr) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Pill, {
							onClick: () => applyPreset(pr.prefix),
							title: pr.prefix,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-3 w-3 opacity-70" }), pr.label]
						}, pr.id))
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "grid gap-3 sm:grid-cols-2 xl:grid-cols-3",
						children: jobs.map((job) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
							className: "overflow-hidden",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "aspect-video bg-[var(--color-surface)]",
								children: job.videoDataUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("video", {
									src: job.videoDataUrl,
									className: "h-full w-full object-cover",
									muted: true
								}) : job.imageDataUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
									src: job.imageDataUrl,
									alt: job.prompt,
									className: "h-full w-full object-cover"
								}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "flex h-full items-center justify-center text-xs text-[var(--color-subtle)]",
									children: job.status === "rendering" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
										className: "inline-flex items-center gap-2",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-3.5 w-3.5 animate-spin" }), "Rendering…"]
									}) : "Queued"
								})
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
								className: "space-y-1 p-3",
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
										className: "flex items-center justify-between gap-2",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
											variant: job.status === "ready" ? "success" : "info",
											children: [
												job.mediaKind || "image",
												" · ",
												job.status
											]
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
											job.quality || "speed",
											job.model ? ` · ${job.model}` : ""
										]
									}),
									job.imageDataUrl && job.status === "ready" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										className: "text-[10px] text-[var(--color-info)] hover:underline",
										onClick: () => {
											setImaginePrompt(job.prompt);
											setImagineAspect(job.aspect);
											if (job.quality) setImagineQuality(job.quality);
											if (job.mediaKind) setImagineMediaKind(job.mediaKind);
										},
										children: "Reuse settings"
									})
								]
							})]
						}, job.id))
					}),
					jobs.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
						className: "py-12 text-center text-sm text-[var(--color-muted)]",
						children: "Type to imagine — pick Image or Video, Speed or Quality, and an aspect ratio."
					}) })
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "shrink-0 rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3 shadow-[0_8px_40px_rgba(0,0,0,0.35)]",
				children: [
					reference && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mb-2 flex items-center gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
								src: reference,
								alt: "Reference",
								className: "h-12 w-12 rounded-lg border border-[var(--color-border)] object-cover"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-xs text-[var(--color-muted)]",
								children: "Reference attached"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "ml-auto rounded-full p-1 text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
								onClick: () => setImagineReference(null),
								"aria-label": "Remove reference",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "h-4 w-4" })
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Textarea, {
						ref: taRef,
						value: prompt,
						onChange: (e) => setImaginePrompt(e.target.value),
						placeholder: "Type to imagine",
						disabled: busy,
						rows: 2,
						className: "min-h-[52px] resize-none border-0 bg-transparent px-1 py-1 text-sm shadow-none focus-visible:ring-0",
						onKeyDown: (e) => {
							if (e.key === "Enter" && !e.shiftKey) {
								e.preventDefault();
								onSubmit();
							}
						}
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mt-2 flex flex-wrap items-center gap-1.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
								ref: fileRef,
								type: "file",
								accept: "image/*",
								className: "hidden",
								onChange: (e) => onPickFile(e.target.files?.[0] || null)
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pill, {
								title: "Attach reference image",
								onClick: () => fileRef.current?.click(),
								disabled: busy,
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "h-3.5 w-3.5" })
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Pill, {
								active: mediaKind === "image",
								onClick: () => setImagineMediaKind("image"),
								disabled: busy,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Image, { className: "h-3.5 w-3.5" }), "Image"]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Pill, {
								active: mediaKind === "video",
								onClick: () => setImagineMediaKind("video"),
								disabled: busy,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Video, { className: "h-3.5 w-3.5" }), "Video"]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pill, {
								active: quality === "speed",
								onClick: () => setImagineQuality("speed"),
								disabled: busy,
								title: "Faster draft",
								children: "Speed"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pill, {
								active: quality === "quality",
								onClick: () => setImagineQuality("quality"),
								disabled: busy,
								title: "Higher fidelity",
								children: "Quality"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "relative",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Pill, {
									active: aspectOpen || aspect !== "auto",
									onClick: () => setAspectOpen((v) => !v),
									disabled: busy,
									title: "Aspect ratio",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Ratio, { className: "h-3.5 w-3.5" }), aspect === "auto" ? "Auto" : aspect]
								}), aspectOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "absolute bottom-full left-0 z-20 mb-2 flex min-w-[10rem] flex-col gap-0.5 rounded-xl border border-[var(--color-border)] bg-[var(--color-elevated)] p-1.5 shadow-xl",
									children: ASPECTS.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										className: cn("rounded-lg px-3 py-1.5 text-left text-xs", a.id === aspect ? "bg-[var(--color-surface)] font-medium" : "text-[var(--color-muted)] hover:bg-[var(--color-surface)]"),
										onClick: () => {
											setImagineAspect(a.id);
											setAspectOpen(false);
										},
										children: a.label
									}, a.id))
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "ml-auto flex items-center gap-1.5",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pill, {
									active: listening,
									onClick: toggleMic,
									disabled: busy,
									title: "Voice prompt",
									children: listening ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MicOff, { className: "h-3.5 w-3.5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Mic, { className: "h-3.5 w-3.5" })
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									disabled: busy || !prompt.trim(),
									onClick: () => void onSubmit(),
									className: cn("inline-flex h-9 w-9 items-center justify-center rounded-full transition-colors", busy || !prompt.trim() ? "bg-[var(--color-elevated)] text-[var(--color-subtle)]" : "bg-[var(--color-fg)] text-[var(--color-bg)] hover:opacity-90"),
									"aria-label": "Generate",
									children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-4 w-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUp, { className: "h-4 w-4" })
								})]
							})
						]
					})
				]
			})
		]
	});
}
//#endregion
export { ImagineView };
