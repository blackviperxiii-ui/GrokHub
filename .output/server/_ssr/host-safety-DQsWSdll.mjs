//#region node_modules/.nitro/vite/services/ssr/assets/host-safety-DQsWSdll.js
/**
* Host command risk classification for confirm-before-run UX.
*/
var SAFE_PREFIXES = [
	"ls",
	"pwd",
	"whoami",
	"uname",
	"date",
	"hostname",
	"id",
	"echo",
	"cat",
	"head",
	"tail",
	"wc",
	"file",
	"stat",
	"find",
	"grep",
	"rg",
	"which",
	"type",
	"df",
	"du",
	"free",
	"uptime",
	"env",
	"printenv",
	"ps",
	"top",
	"realpath",
	"readlink",
	"tree",
	"git status",
	"git log",
	"git diff",
	"git branch",
	"git show",
	"npm ls",
	"npm list",
	"node -v",
	"python --version",
	"python3 --version"
];
var DESTRUCTIVE_PATTERNS = [
	/\brm\s+(-[a-zA-Z]*f|[^\n]*\s-rf|\s-fr)/i,
	/\brm\s+-[a-zA-Z]*r/i,
	/\bmkfs\b/i,
	/\bdd\s+if=/i,
	/\b(shutdown|reboot|poweroff|halt)\b/i,
	/\bchmod\s+-R\s+777\b/i,
	/\bchown\s+-R\b/i,
	/>\s*\/dev\/sd/i,
	/\bcurl\b.*\|\s*(ba)?sh\b/i,
	/\bwget\b.*\|\s*(ba)?sh\b/i,
	/\bsudo\b/i,
	/\bsystemctl\s+(stop|disable|mask)\b/i,
	/\bpacman\s+-R/i,
	/\bapt(-get)?\s+remove\b/i,
	/\bnpm\s+publish\b/i,
	/\bgit\s+push\s+.*--force\b/i,
	/\bgit\s+reset\s+--hard\b/i,
	/\btruncate\b/i,
	/\bshred\b/i
];
function firstToken(cmd) {
	return cmd.trim().replace(/^\$\s*/, "").replace(/^(\w+=\S+\s+)+/, "").split(/\s+/)[0]?.toLowerCase() || "";
}
function classifyHostCommand(cmd) {
	const raw = cmd.trim();
	if (!raw) return "safe";
	for (const re of DESTRUCTIVE_PATTERNS) if (re.test(raw)) return "destructive";
	const lower = raw.toLowerCase();
	for (const p of SAFE_PREFIXES) if (lower === p || lower.startsWith(p + " ") || lower.startsWith(p + "	")) return "safe";
	if (/[|;]|\s>>?|\s<|&&|\|\|/.test(raw) && !/^\s*(ls|cat|head|tail|grep|rg)\b/i.test(raw)) return "moderate";
	const tok = firstToken(raw);
	if ([
		"rm",
		"mv",
		"cp",
		"chmod",
		"chown",
		"kill",
		"pkill",
		"killall",
		"dd"
	].includes(tok)) return "destructive";
	if ([
		"mkdir",
		"touch",
		"tee",
		"sed",
		"awk",
		"npm",
		"pip",
		"cargo",
		"make"
	].includes(tok)) return "moderate";
	return "moderate";
}
function needsHostConfirm(cmds, opts) {
	if (!cmds.length) return false;
	if (opts.confirmAll) return true;
	if (!opts.confirmDestructive) return false;
	return cmds.some((c) => classifyHostCommand(c) !== "safe");
}
function riskLabel(risk) {
	if (risk === "destructive") return "destructive";
	if (risk === "moderate") return "writes / side effects";
	return "read-only";
}
//#endregion
export { classifyHostCommand, needsHostConfirm, riskLabel };
