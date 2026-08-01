import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-CfSUHW0g.mjs";
import { I as FolderInput, _ as Plus, l as Sparkles, v as Play } from "../_libs/lucide-react.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
import { t as Textarea } from "./textarea-aSaPOSKs.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/SkillsView-DEUET5hZ.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function SkillsView() {
	const skills = useGrokHub((s) => s.skills);
	const toggleSkill = useGrokHub((s) => s.toggleSkill);
	const runSkill = useGrokHub((s) => s.runSkill);
	const addSkill = useGrokHub((s) => s.addSkill);
	const running = useGrokHub((s) => s.running);
	const importOpenClawWorkspace = useGrokHub((s) => s.importOpenClawWorkspace);
	const openClawWorkspace = useGrokHub((s) => s.openClawWorkspace);
	const [name, setName] = (0, import_react.useState)("");
	const [slash, setSlash] = (0, import_react.useState)("");
	const [description, setDescription] = (0, import_react.useState)("");
	const [instructions, setInstructions] = (0, import_react.useState)("");
	const [ocPath, setOcPath] = (0, import_react.useState)("~/.openclaw/workspace");
	const [ocBusy, setOcBusy] = (0, import_react.useState)(false);
	const [ocDetail, setOcDetail] = (0, import_react.useState)("");
	function onCreate() {
		if (!name.trim() || !instructions.trim()) return;
		addSkill({
			name: name.trim(),
			slash: slash.trim() || `/${name.trim().toLowerCase().replace(/\s+/g, "-")}`,
			description: description.trim() || "Custom skill",
			instructions: instructions.trim()
		});
		setName("");
		setSlash("");
		setDescription("");
		setInstructions("");
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
			className: "flex items-center gap-2 text-sm",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderInput, { className: "h-4 w-4 text-[var(--color-muted)]" }), "Import from OpenClaw"]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
			"Load workspace skills from",
			" ",
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "font-mono",
				children: "~/.openclaw/workspace/skills"
			}),
			" (or another path)."
		] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "flex flex-wrap items-center gap-2",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					value: ocPath,
					onChange: (e) => setOcPath(e.target.value),
					className: "min-w-[200px] max-w-md flex-1 font-mono text-xs",
					placeholder: "~/.openclaw/workspace"
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					disabled: ocBusy,
					onClick: () => {
						setOcBusy(true);
						setOcDetail("");
						importOpenClawWorkspace(ocPath.trim() || void 0).then((r) => {
							setOcDetail(r.detail);
							setOcBusy(false);
						});
					},
					children: ocBusy ? "Importing…" : "Import workspace"
				}),
				openClawWorkspace && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
					variant: "success",
					children: [
						openClawWorkspace.identityName || "Linked",
						" ·",
						" ",
						openClawWorkspace.filesImported.length,
						" files"
					]
				}),
				ocDetail && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "w-full text-xs text-[var(--color-muted)]",
					children: ocDetail
				})
			]
		})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "grid gap-4 lg:grid-cols-[1fr_340px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-3 sm:grid-cols-2",
				children: skills.map((sk) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "flex flex-col",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start justify-between gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
							className: "flex items-center gap-2 text-sm",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-3.5 w-3.5 text-[var(--color-muted)]" }), sk.name]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
							className: "mt-1 font-mono text-xs",
							children: sk.slash
						})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
							variant: sk.kind === "builtin" ? "info" : "default",
							children: [sk.kind, sk.id.startsWith("ocskill") ? " · openclaw" : ""]
						})]
					}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "mt-auto flex flex-1 flex-col gap-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm text-[var(--color-muted)]",
								children: sk.description
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "line-clamp-2 text-xs text-[var(--color-subtle)]",
								children: sk.instructions
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-auto flex items-center justify-between gap-2 pt-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "tabular text-xs text-[var(--color-subtle)]",
									children: [sk.runs, " runs"]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "flex gap-2",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
										size: "sm",
										variant: "secondary",
										onClick: () => toggleSkill(sk.id),
										children: sk.enabled ? "Disable" : "Enable"
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
										size: "sm",
										disabled: !sk.enabled || running,
										onClick: () => void runSkill(sk.id),
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "h-3.5 w-3.5" }), "Run"]
									})]
								})]
							})
						]
					})]
				}, sk.id))
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "h-fit lg:sticky lg:top-20",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "h-4 w-4" }), "Skill Creator"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Teach once — slash command sticks across sessions." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
					className: "space-y-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Name"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: name,
								onChange: (e) => setName(e.target.value),
								placeholder: "Gig day planner"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Slash"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: slash,
								onChange: (e) => setSlash(e.target.value),
								placeholder: "/gigs"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Description"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: description,
								onChange: (e) => setDescription(e.target.value),
								placeholder: "What this skill does"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Instructions"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Textarea, {
								value: instructions,
								onChange: (e) => setInstructions(e.target.value),
								placeholder: "Steps the agent should always follow…"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							className: "w-full",
							onClick: onCreate,
							disabled: !name.trim() || !instructions.trim(),
							children: "Save skill"
						})
					]
				})]
			})]
		})]
	});
}
//#endregion
export { SkillsView };
