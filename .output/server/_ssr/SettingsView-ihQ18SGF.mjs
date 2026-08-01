import { o as __toESM } from "../_runtime.mjs";
import { d as friendlyModelName, p as getModesWithCatalog } from "./version-BE4o4tL_.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { l as useCurrentUserState, r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, i as CardDescription, n as Card, o as CardTitle, r as CardContent, s as useGrokHub, t as Badge } from "./card-J2wZc2w_.mjs";
import { M as FolderInput, N as ExternalLink } from "../_libs/lucide-react.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
import { t as HostGatewayBanner } from "./HostGatewayBanner-CUCDDmX3.mjs";
import { n as UsageMeterPanel } from "./UsageMeter-CY9tl2wp.mjs";
import { applyUpdate, checkUpdate } from "./grok-client-DNfOOpUN.mjs";
import { t as ProfileAvatar } from "./ProfileAvatar-DhCtn6_K.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/SettingsView-ihQ18SGF.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function SettingsView() {
	const mode = useGrokHub((s) => s.mode);
	const setMode = useGrokHub((s) => s.setMode);
	const modelCatalog = useGrokHub((s) => s.modelCatalog);
	const refreshModels = useGrokHub((s) => s.refreshModels);
	const lastModelsFetchAt = useGrokHub((s) => s.lastModelsFetchAt);
	const desktop = useGrokHub((s) => s.desktop);
	const setDesktop = useGrokHub((s) => s.setDesktop);
	const setNav = useGrokHub((s) => s.setNav);
	const resetDemo = useGrokHub((s) => s.resetDemo);
	const apiKey = useGrokHub((s) => s.apiKey);
	const setApiKey = useGrokHub((s) => s.setApiKey);
	const githubToken = useGrokHub((s) => s.githubToken);
	const setGithubToken = useGrokHub((s) => s.setGithubToken);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
	const probeGrok = useGrokHub((s) => s.probeGrok);
	const syncFromGrok = useGrokHub((s) => s.syncFromGrok);
	useGrokHub((s) => s.profile);
	const oauth = useGrokHub((s) => s.oauth);
	const oauthPending = useGrokHub((s) => s.oauthPending);
	const startGrokOAuth = useGrokHub((s) => s.startGrokOAuth);
	const pollGrokOAuth = useGrokHub((s) => s.pollGrokOAuth);
	const clearGrokOAuth = useGrokHub((s) => s.clearGrokOAuth);
	const importOpenClawWorkspace = useGrokHub((s) => s.importOpenClawWorkspace);
	const clearOpenClawWorkspace = useGrokHub((s) => s.clearOpenClawWorkspace);
	const openClawWorkspace = useGrokHub((s) => s.openClawWorkspace);
	const { user } = useCurrentUserState();
	const [keyDraft, setKeyDraft] = (0, import_react.useState)(apiKey);
	const [ghDraft, setGhDraft] = (0, import_react.useState)(githubToken);
	const [probing, setProbing] = (0, import_react.useState)(false);
	const [oauthBusy, setOauthBusy] = (0, import_react.useState)(false);
	const [oauthErr, setOauthErr] = (0, import_react.useState)("");
	const [update, setUpdate] = (0, import_react.useState)(null);
	const [updateBusy, setUpdateBusy] = (0, import_react.useState)(false);
	const [updateLog, setUpdateLog] = (0, import_react.useState)("");
	const [ocPath, setOcPath] = (0, import_react.useState)("~/.openclaw/workspace");
	const [ocBusy, setOcBusy] = (0, import_react.useState)(false);
	const [ocDetail, setOcDetail] = (0, import_react.useState)("");
	const pollRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		setKeyDraft(apiKey);
	}, [apiKey]);
	(0, import_react.useEffect)(() => {
		setGhDraft(githubToken);
	}, [githubToken]);
	(0, import_react.useEffect)(() => {
		if (!oauth?.accessToken) return;
		let cancelled = false;
		(async () => {
			setProbing(true);
			try {
				await probeGrok();
			} finally {
				if (!cancelled) setProbing(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [oauth?.accessToken, oauth?.email]);
	(0, import_react.useEffect)(() => {
		if (!oauthPending) {
			if (pollRef.current) {
				window.clearInterval(pollRef.current);
				pollRef.current = null;
			}
			return;
		}
		const tick = async () => {
			try {
				const r = await pollGrokOAuth();
				if (r === "ready" || r === "failed") setOauthBusy(false);
			} catch (e) {
				setOauthErr(e instanceof Error ? e.message : "poll failed");
				setOauthBusy(false);
			}
		};
		tick();
		pollRef.current = window.setInterval(() => void tick(), 5e3);
		return () => {
			if (pollRef.current) window.clearInterval(pollRef.current);
		};
	}, [oauthPending, pollGrokOAuth]);
	(0, import_react.useEffect)(() => {
		checkUpdate(githubToken || void 0).then(setUpdate).catch((e) => setUpdate({
			currentVersion: "0.2.0",
			currentSha: null,
			remoteSha: null,
			remoteMessage: null,
			updateAvailable: false,
			repo: "blackviperxiii-ui/Grok-Hub",
			branch: "main",
			installRoot: null,
			detail: e instanceof Error ? e.message : "check failed"
		}));
	}, [githubToken]);
	async function onStartOAuth() {
		setOauthErr("");
		setOauthBusy(true);
		try {
			await startGrokOAuth();
			const pending = useGrokHub.getState().oauthPending;
			if (pending?.verificationUriComplete) window.open(pending.verificationUriComplete, "_blank", "noopener,noreferrer");
			else if (pending?.verificationUri) window.open(pending.verificationUri, "_blank", "noopener,noreferrer");
		} catch (e) {
			setOauthErr(e instanceof Error ? e.message : "Could not start OAuth");
			setOauthBusy(false);
		}
	}
	async function saveAndProbe() {
		setApiKey(keyDraft.trim());
		setProbing(true);
		try {
			if (await probeGrok() && user && !user.isDevFallback) await syncFromGrok({
				displayName: user.displayName,
				email: user.primaryEmail,
				imageUrl: user.profileImageUrl
			});
		} finally {
			setProbing(false);
		}
	}
	async function onCheckUpdate() {
		setUpdateBusy(true);
		setUpdateLog("");
		try {
			setGithubToken(ghDraft.trim());
			const s = await checkUpdate(ghDraft.trim() || void 0);
			setUpdate(s);
			setUpdateLog(s.detail);
		} catch (e) {
			setUpdateLog(e instanceof Error ? e.message : "check failed");
		} finally {
			setUpdateBusy(false);
		}
	}
	async function onInstallUpdate() {
		setUpdateBusy(true);
		setUpdateLog("Installing update from GitHub…");
		try {
			setGithubToken(ghDraft.trim());
			const r = await applyUpdate(ghDraft.trim() || void 0, true);
			if (r.status) setUpdate(r.status);
			else {
				const s = await checkUpdate(ghDraft.trim() || void 0);
				setUpdate(s);
			}
			const lines = [
				r.ok ? "OK" : "FAILED",
				r.detail,
				r.newVersion ? `Version: v${r.newVersion}` : "",
				r.newSha ? `Commit: ${r.newSha}` : "",
				"",
				...r.steps || []
			].filter(Boolean);
			if (r.ok && r.restarting) {
				lines.push("", "Restarting GrokHub…");
				setUpdateLog(lines.join("\n"));
				setTimeout(() => {
					window.location.reload();
				}, 1500);
				return;
			}
			setUpdateLog(lines.join("\n"));
		} catch (e) {
			setUpdateLog(e instanceof Error ? e.message : "update failed");
		} finally {
			setUpdateBusy(false);
		}
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "content-readable mx-auto space-y-5 pb-8",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Connect to Grok (xAI OAuth)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Sign in with your ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: "SuperGrok" }),
				" or ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: "X Premium+" }),
				" account via xAI device code — same flow as OpenClaw / Grok CLI. No API key required for subscription access."
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: oauth && grokConnected ? "success" : oauth ? "info" : grokConnected ? "success" : "default",
								children: oauth && grokConnected ? "OAuth live" : oauth ? "OAuth session" : grokConnected ? "API connected" : "Not connected"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-xs text-[var(--color-muted)]",
								children: probing ? "Verifying with xAI…" : oauth && grokStatusDetail.toLowerCase().includes("not connected") ? "Session saved — verifying API access…" : grokStatusDetail
							}),
							oauth && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "secondary",
								size: "sm",
								disabled: probing,
								onClick: () => void saveAndProbe(),
								children: probing ? "Testing…" : "Test connection"
							})
						]
					}),
					oauth && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-3 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProfileAvatar, {
								src: oauth.picture,
								name: oauth.name,
								email: oauth.email
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0 flex-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium",
									children: oauth.name || "Grok account"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "truncate text-xs text-[var(--color-muted)]",
									children: oauth.email || "OAuth session active"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "secondary",
								size: "sm",
								onClick: () => clearGrokOAuth(),
								children: "Disconnect"
							})
						]
					}),
					oauthPending && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-3 rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] p-4",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs uppercase tracking-wide text-[var(--color-subtle)]",
								children: "Approve this code"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "font-mono text-3xl font-semibold tracking-[0.2em] text-[var(--color-fg)]",
								children: oauthPending.userCode
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm text-[var(--color-muted)]",
								children: "Open the link, sign in to xAI / Grok, and enter the code. This window polls automatically."
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex flex-wrap gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
									onClick: () => window.open(oauthPending.verificationUriComplete || oauthPending.verificationUri, "_blank", "noopener,noreferrer"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "h-4 w-4" }), "Open accounts.x.ai"]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
									variant: "secondary",
									onClick: () => void pollGrokOAuth(),
									children: "Check now"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "text-xs text-[var(--color-subtle)]",
								children: ["Waiting for approval… ", oauthBusy ? "polling" : ""]
							})
						]
					}),
					!oauth && !oauthPending && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						onClick: () => void onStartOAuth(),
						disabled: oauthBusy,
						children: oauthBusy ? "Starting…" : "Connect with Grok OAuth"
					}),
					oauthErr && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-sm text-[var(--color-danger)]",
						children: oauthErr
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Uses xAI public OAuth client (device code). Tokens stay on this device only and are never committed to git."
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterPanel, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderInput, { className: "h-4 w-4 text-[var(--color-muted)]" }), "Import OpenClaw workspace"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Pull skills, persona, and memory from an OpenClaw agent home (",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-mono",
					children: "~/.openclaw/workspace"
				}),
				"). Credentials and sqlite sessions are not imported."
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					openClawWorkspace ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-2 text-sm",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "font-medium",
								children: openClawWorkspace.identityName || "Workspace linked"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "truncate font-mono text-xs text-[var(--color-muted)]",
								children: openClawWorkspace.root
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-1 text-xs text-[var(--color-subtle)]",
								children: [
									openClawWorkspace.filesImported.length,
									" files · imported",
									" ",
									new Date(openClawWorkspace.importedAt).toLocaleString()
								]
							})
						]
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Default path is scanned automatically if you leave it blank."
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: ocPath,
								onChange: (e) => setOcPath(e.target.value),
								placeholder: "~/.openclaw/workspace",
								className: "min-w-[220px] flex-1 font-mono text-xs"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
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
							openClawWorkspace && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "secondary",
								onClick: () => clearOpenClawWorkspace(),
								children: "Clear import"
							})
						]
					}),
					ocDetail && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-[var(--color-muted)]",
						children: ocDetail
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "text-[11px] text-[var(--color-subtle)]",
						children: [
							"Imports ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "font-mono",
								children: "skills/**/SKILL.md"
							}),
							", AGENTS/SOUL/USER/IDENTITY, HEARTBEAT → automation, and injects context into Agent chat."
						]
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "xAI API key (optional fallback)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Pay-per-token console key if you are not using SuperGrok OAuth. From",
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-mono",
					children: "console.x.ai"
				}),
				"."
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					type: "password",
					autoComplete: "off",
					value: keyDraft,
					onChange: (e) => setKeyDraft(e.target.value),
					placeholder: "xai-…"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						onClick: () => void saveAndProbe(),
						disabled: probing,
						children: probing ? "Testing…" : "Save & test key"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "secondary",
						onClick: () => {
							setKeyDraft("");
							setApiKey("");
						},
						children: "Clear key"
					})]
				})]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-start justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
					className: "text-sm",
					children: "Essential models"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Polled from xAI · only 4.5 / 4.3 / Fast / Build / Imagine class ids" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					variant: "secondary",
					onClick: () => void refreshModels({ force: true }),
					children: "Refresh + reclassify"
				})]
			}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "grid gap-2 sm:grid-cols-2",
						children: [
							["fast", "Fast chat"],
							["balanced", "Balanced"],
							["smart", "Brains"],
							["heavy", "Heavy / team"],
							["build", "Build / code"],
							["imagine", "Imagine"]
						].map(([slot, label]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-[10px] uppercase tracking-wide text-[var(--color-subtle)]",
									children: label
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium text-[var(--color-fg)]",
									children: friendlyModelName(modelCatalog.slots[slot])
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "font-mono text-[10px] text-[var(--color-muted)]",
									children: modelCatalog.slots[slot]
								})
							]
						}, slot))
					}),
					modelCatalog.essential.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-1",
						children: modelCatalog.essential.map((m) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							className: "font-mono text-[10px]",
							children: m
						}, m))
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "text-[10px] text-[var(--color-subtle)]",
						children: [
							"Source: ",
							modelCatalog.source,
							" · slots by",
							" ",
							modelCatalog.classifiedBy === "grok" ? "Grok" : "heuristic",
							modelCatalog.classifyNotes ? ` · ${modelCatalog.classifyNotes}` : "",
							lastModelsFetchAt ? ` · last poll ${new Date(lastModelsFetchAt).toLocaleTimeString()}` : " · not polled yet"
						]
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Updates (GitHub)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Install the latest clean release from the package repo." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					update && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex flex-wrap items-center gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, { children: ["v", update.currentVersion] }), update.updateAvailable ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: "info",
								children: "Update available"
							}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: "success",
								children: "Up to date"
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-2 space-y-1 font-mono text-xs text-[var(--color-muted)]",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: update.detail }), (update.currentSha || update.remoteSha) && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
								"local ",
								update.currentSha || "?",
								update.remoteSha ? ` · remote ${update.remoteSha}` : ""
							] })]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						type: "password",
						autoComplete: "off",
						value: ghDraft,
						onChange: (e) => setGhDraft(e.target.value),
						placeholder: "GitHub token (optional)"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							disabled: updateBusy,
							onClick: () => void onCheckUpdate(),
							children: "Check for updates"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							disabled: updateBusy,
							onClick: () => void onInstallUpdate(),
							children: updateBusy ? "Installing…" : update?.updateAvailable ? "Install latest" : "Reinstall / repair"
						})]
					}),
					updateLog && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
						className: "scroll-panel max-h-48 whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-3 font-mono text-xs text-[var(--color-muted)]",
						children: updateLog
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Model modes"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Auto routes each prompt to Fast · 4.3 · 4.5 · Build · Imagine balancing tokens." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-2",
				children: getModesWithCatalog(modelCatalog).map((m) => {
					const selected = m.id === mode;
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => setMode(m.id),
						className: cn("flex w-full items-center justify-between gap-3 rounded-[var(--radius-md)] border px-3 py-3 text-left transition-colors", selected ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]" : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-sm font-medium",
								children: m.label
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs text-[var(--color-muted)]",
								children: m.subtitle
							}),
							m.id !== "auto" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mt-0.5 font-mono text-[10px] text-[var(--color-subtle)]",
								children: m.modelId
							})
						] }), selected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: "Active"
						})]
					}, m.id);
				})
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HostGatewayBanner, {
				variant: "card",
				onOpenDesktop: () => setNav("desktop")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Arch Linux shell preferences"
			}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-3",
				children: [
					[
						"wayland",
						"Prefer Wayland",
						"Ozone flags"
					],
					[
						"tray",
						"System tray",
						"Minimize to tray"
					],
					[
						"launchOnLogin",
						"Launch on login",
						"Autostart"
					],
					[
						"startMinimized",
						"Start minimized",
						"Tray only"
					],
					[
						"confirmHostCommands",
						"Confirm host commands",
						"Ask before agent runs shell on your machine"
					],
					[
						"confirmDestructiveOnly",
						"Only risky commands",
						"Skip confirm for read-only commands (ls, cat, …)"
					]
				].map(([key, label, hint]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
					className: "flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-sm font-medium",
						children: label
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-xs text-[var(--color-muted)]",
						children: hint
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
						type: "checkbox",
						className: "h-4 w-4 accent-[var(--color-fg)]",
						checked: Boolean(desktop[key]),
						onChange: (e) => setDesktop({ [key]: e.target.checked })
					})]
				}, key))
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Danger zone"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Wipe local chat history, connectors, and preferences on this device. Does not revoke Grok OAuth on xAI servers — disconnect first if you want a full sign-out." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "secondary",
				onClick: () => {
					if (typeof window !== "undefined" && window.confirm("Reset GrokHub to a clean install on this device?")) resetDemo();
				},
				children: "Reset to clean install"
			}) })] })
		]
	});
}
//#endregion
export { SettingsView };
