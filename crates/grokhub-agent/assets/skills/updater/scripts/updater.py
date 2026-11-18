#!/usr/bin/env python3
"""Find every part of an app that needs updating and make the safe bumps.

Stdlib only. Subcommands:

  run      inventory + check + report (default)
  bump     apply the safe ("auto") findings for one area

The model reads report.md; everything else is for scripts. Output goes to
--out (default: $XDG_CACHE_HOME/vstack-updater/<repo name>), never the repo.

Sources the shell cannot reach are listed under "Unchecked sources" with a
file name. Fetch that URL with any other tool, save the raw body under
<out>/fetched/<file name>, and rerun: the saved body is used instead.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import asdict, dataclass, field
from pathlib import Path

USER_AGENT = "vstack-updater (+https://github.com/blackviperxiii-ui/vstack)"
SKIP_DIRS = {
    ".git", "node_modules", "target", "dist", "build", ".venv", "venv",
    "__pycache__", ".next", ".turbo", ".cache", ".updater", "out",
}
SOURCE_EXTS = {
    ".rs", ".ts", ".tsx", ".js", ".mjs", ".cjs", ".py", ".go", ".lua", ".cs",
    ".kt", ".java", ".swift", ".toml", ".json", ".yml", ".yaml", ".cpp", ".c", ".h",
}
LOCKFILES = {"Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "poetry.lock"}
MAX_FILE_BYTES = 1_000_000
SEVERITY_ORDER = {"critical": 0, "high": 1, "medium": 2, "low": 3, "info": 4}

# Detectors: markers that reveal a support surface. They decide what to look
# at; the list of surfaces itself always comes from the repo.
MODEL_RE = re.compile(
    r"[\"'`]((?:grok|gpt|claude|gemini|gemma|llama|mistral|mixtral|codestral|qwen|deepseek|command-r)"
    r"-[A-Za-z0-9][\w.:-]*)[\"'`]"
)
MODEL_FAMILY = {
    "grok": "xai", "gpt": "openai", "claude": "anthropic", "gemini": "google",
    "gemma": "google", "llama": "meta", "mistral": "mistral", "mixtral": "mistral",
    "codestral": "mistral", "qwen": "qwen", "deepseek": "deepseek", "command-r": "cohere",
}
OPENROUTER_PREFIX = {
    "xai": "x-ai/", "openai": "openai/", "anthropic": "anthropic/", "google": "google/",
    "meta": "meta-llama/", "mistral": "mistralai/", "qwen": "qwen/", "deepseek": "deepseek/",
    "cohere": "cohere/",
}
OFFICIAL_MODEL_LISTS = {
    # family: (env var, url, header builder)
    "xai": ("XAI_API_KEY", "https://api.x.ai/v1/models", lambda k: {"Authorization": f"Bearer {k}"}),
    "openai": ("OPENAI_API_KEY", "https://api.openai.com/v1/models", lambda k: {"Authorization": f"Bearer {k}"}),
    "anthropic": ("ANTHROPIC_API_KEY", "https://api.anthropic.com/v1/models",
                  lambda k: {"x-api-key": k, "anthropic-version": "2023-06-01"}),
}
ENDPOINT_RE = re.compile(r"https://((?:[\w-]+\.)*(?:api|[\w-]*-api)\.[\w.-]+|api\.[\w.-]+)(/v\d+[\w.]*)?")
LAUNCHER_MARKERS = {
    "Steam": ("steamapps", "appmanifest_", "libraryfolders.vdf"),
    "Battle.net": (".build.info", "Battle.net"),
    "Riot Client": ("RiotClient", "Riot Games"),
    "Epic Games": ("LauncherInstalled.dat", "EpicGamesLauncher"),
    "GOG Galaxy": ("GOG Galaxy", "galaxy-2.0"),
    "EA app": ("EA Desktop", "EADesktop"),
    "Ubisoft Connect": ("Ubisoft Game Launcher",),
    "Heroic": ("heroic/", "HeroicGamesLauncher"),
    "Lutris": ("lutris",),
    "Minecraft": ("level.dat", ".minecraft"),
    "Valve game state integration": ("gamestate_integration",),
}
MOD_MARKERS = {
    "Nexus Mods": ("nexusmods",), "CurseForge": ("curseforge",), "Modrinth": ("modrinth",),
    "Thunderstore": ("thunderstore",), "Steam Workshop": ("sharedfiles/filedetails", "steamworkshop"),
}
MOD_SOURCES = {
    "Nexus Mods": "https://api.nexusmods.com/v1/games.json",
    "CurseForge": "https://api.curseforge.com/v1/games",
    "Modrinth": "https://api.modrinth.com/v2/tag/game_version",
    "Thunderstore": "https://thunderstore.io/api/v1/package/",
}
ANTICHEAT_RE = re.compile(
    r"\b(EasyAntiCheat|Easy Anti-Cheat|BattlEye|Vanguard|FACEIT|Ricochet|nProtect|GameGuard|"
    r"XIGNCODE|Denuvo Anti-Cheat|Warden|EAC)\b"
)
WOW_PRODUCTS = {
    "wow": "retail", "wow_classic": "classic", "wow_classic_era": "classic era",
    "wow_anniversary": "anniversary",
}
I18N_DIRS = {"locales", "locale", "i18n", "lang", "langs", "translations"}


# ---------------------------------------------------------------- versions

VERSION_RE = re.compile(r"v?(\d+)(?:\.(\d+))?(?:\.(\d+))?([-+][\w.+-]*)?$")


def parse_version(text: str) -> tuple[int, int, int] | None:
    """'v1.2' -> (1, 2, 0). Pre-releases and junk -> None."""
    m = VERSION_RE.match(text.strip())
    if not m or (m.group(4) and m.group(4).startswith("-")):
        return None
    return int(m.group(1)), int(m.group(2) or 0), int(m.group(3) or 0)


def bump_kind(current: tuple[int, int, int], latest: tuple[int, int, int]) -> str:
    """none, patch, minor or major, with Cargo's rule that 0.x minors break."""
    if latest <= current:
        return "none"
    if latest[0] != current[0]:
        return "major"
    if current[0] == 0 and latest[1] != current[1]:
        return "major"
    if latest[1] != current[1]:
        return "minor"
    return "patch"


def req_base(req: str) -> tuple[int, int, int] | None:
    """Lowest version a requirement names: '^1.2', '=1.2.3', '>=1, <2', '1'."""
    m = re.search(r"\d+(?:\.\d+){0,2}", req)
    return parse_version(m.group(0)) if m else None


def _cmp_one(version: tuple[int, int, int], clause: str) -> bool:
    clause = clause.strip()
    m = re.match(r"(>=|<=|>|<|=|\^|~)?\s*v?(\d+(?:\.\d+){0,2})(?:\.\*|-[\w.]+)?$", clause)
    if not m:
        return False
    op, raw = m.group(1) or "^", m.group(2)
    parts = [int(p) for p in raw.split(".")]
    bound = tuple(parts + [0] * (3 - len(parts)))
    if op == ">=":
        return version >= bound
    if op == ">":
        return version > bound
    if op == "<=":
        return version <= bound
    if op == "<":
        return version < bound
    if op == "=":
        return version[: len(parts)] == bound[: len(parts)]
    if op == "~":
        upper = (bound[0] + 1, 0, 0) if len(parts) == 1 else (bound[0], bound[1] + 1, 0)
        return bound <= version < upper
    # caret
    if bound[0] > 0 or len(parts) == 1:
        upper = (bound[0] + 1, 0, 0)
    elif bound[1] > 0 or len(parts) == 2:
        upper = (0, bound[1] + 1, 0)
    else:
        upper = (0, 0, bound[2] + 1)
    return bound <= version < upper


def matches_req(version: tuple[int, int, int], req: str) -> bool:
    """Cargo-style requirement; commas mean AND."""
    return all(_cmp_one(version, c) for c in req.split(",") if c.strip())


def wow_interface(build: str) -> int | None:
    """'11.2.7.64978' -> 110207 (the number a .toc's ## Interface uses)."""
    m = re.match(r"(\d+)\.(\d+)\.(\d+)", build)
    if not m:
        return None
    major, minor, patch = (int(x) for x in m.groups())
    return major * 10000 + minor * 100 + patch


# ---------------------------------------------------------------- model

@dataclass
class Surface:
    area: str
    kind: str
    name: str
    current: str = ""
    files: list[str] = field(default_factory=list)
    extra: dict = field(default_factory=dict)


@dataclass
class Finding:
    area: str
    surface: str
    severity: str
    action: str  # auto, card, manual, watch, guarded
    current: str = ""
    latest: str = ""
    note: str = ""
    files: list[str] = field(default_factory=list)
    data: dict = field(default_factory=dict)


# ---------------------------------------------------------------- inventory

def walk(repo: Path):
    for root, dirs, files in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS and not d.startswith(".") or d in {".github"}]
        for name in files:
            yield Path(root) / name


def rel(repo: Path, path: Path) -> str:
    return path.relative_to(repo).as_posix()


def _cargo_deps(doc: dict):
    tables = []
    for key in ("dependencies", "dev-dependencies", "build-dependencies"):
        tables.append(doc.get(key, {}))
        tables.append(doc.get("workspace", {}).get(key, {}))
    for target in doc.get("target", {}).values():
        for key in ("dependencies", "dev-dependencies", "build-dependencies"):
            tables.append(target.get(key, {}))
    for table in tables:
        for name, spec in table.items():
            if isinstance(spec, str):
                yield name, spec
            elif isinstance(spec, dict) and "version" in spec and not spec.get("git") and not spec.get("path"):
                yield spec.get("package", name), spec["version"]


def inventory(repo: Path) -> list[Surface]:
    found: dict[tuple[str, str, str], Surface] = {}

    def add(area: str, kind: str, name: str, current: str, path: Path, **extra) -> Surface:
        key = (area, kind, name)
        s = found.get(key)
        if s is None:
            s = found[key] = Surface(area, kind, name, current, [], dict(extra))
        r = rel(repo, path)
        if r not in s.files:
            s.files.append(r)
        if current and current not in s.current.split(" | "):
            s.current = f"{s.current} | {current}" if s.current else current
        for k, v in extra.items():
            s.extra.setdefault(k, v)
        return s

    i18n_files: dict[Path, list[Path]] = {}
    for path in walk(repo):
        name = path.name
        try:
            size = path.stat().st_size
        except OSError:
            continue
        if size > MAX_FILE_BYTES:
            continue
        if name == "Cargo.toml":
            try:
                doc = tomllib.loads(path.read_text(encoding="utf-8"))
            except (tomllib.TOMLDecodeError, UnicodeDecodeError):
                continue
            for dep, req in _cargo_deps(doc):
                add("deps", "cargo", dep, req, path)
            for tbl in (doc.get("package", {}), doc.get("workspace", {}).get("package", {})):
                rv = tbl.get("rust-version")
                if isinstance(rv, str):
                    add("toolchain", "rust-msrv", "rust-version", rv, path)
        elif name == "Cargo.lock":
            add("deps", "cargo-lock", rel(repo, path), "", path)
        elif name in ("rust-toolchain.toml", "rust-toolchain"):
            text = path.read_text(encoding="utf-8", errors="replace")
            m = re.search(r'channel\s*=\s*"([^"]+)"', text) or re.match(r"\s*(\S+)", text)
            add("toolchain", "rust-toolchain", "rust", m.group(1) if m else "", path)
        elif name == "package.json":
            try:
                doc = json.loads(path.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, UnicodeDecodeError):
                continue
            for key in ("dependencies", "devDependencies", "optionalDependencies"):
                for dep, spec in (doc.get(key) or {}).items():
                    if isinstance(spec, str) and re.match(r"^[\^~=]?v?\d", spec):
                        add("deps", "npm", dep, spec, path, manifest=rel(repo, path))
            node = (doc.get("engines") or {}).get("node")
            if node:
                add("toolchain", "node-engine", "node", node, path)
            if (path.parent / "package-lock.json").is_file():
                add("deps", "npm-lock", rel(repo, path.parent / "package-lock.json"), "", path.parent / "package-lock.json")
        elif name in (".nvmrc", ".node-version"):
            add("toolchain", "node-pin", "node", path.read_text(encoding="utf-8").strip(), path)
        elif re.match(r"requirements.*\.txt$", name):
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                m = re.match(r"\s*([A-Za-z0-9_.\-\[\]]+)\s*==\s*([\w.]+)", line)
                if m:
                    add("deps", "pypi", re.sub(r"\[.*\]", "", m.group(1)), m.group(2), path)
        elif path.suffix in (".yml", ".yaml") and (".github/workflows" in rel(repo, path) or name in ("action.yml", "action.yaml")):
            text = path.read_text(encoding="utf-8", errors="replace")
            for m in re.finditer(r"uses:\s*['\"]?([\w.-]+/[\w.-]+)(/[^@\s'\"]*)?@([^\s#'\"]+)", text):
                add("ci", "action", m.group(1), m.group(3), path)
        elif path.suffix == ".toc":
            text = path.read_text(encoding="utf-8", errors="replace")
            nums = []
            for m in re.finditer(r"^##\s*Interface(?:-\w+)?:\s*([\d,\s]+)$", text, re.M):
                nums += [int(n) for n in re.findall(r"\d+", m.group(1))]
            if nums:
                add("addons", "wow-toc", rel(repo, path), ",".join(str(n) for n in nums), path)
        elif name == "fabric.mod.json":
            try:
                doc = json.loads(path.read_text(encoding="utf-8"))
                mc = (doc.get("depends") or {}).get("minecraft", "")
            except (json.JSONDecodeError, UnicodeDecodeError):
                mc = ""
            add("addons", "minecraft-mod", rel(repo, path), str(mc), path)
        elif name == "pack.mcmeta":
            add("addons", "minecraft-pack", rel(repo, path), "", path)

        if path.parent.name.lower() in I18N_DIRS or path.parent.parent.name.lower() in I18N_DIRS:
            if path.suffix in (".json", ".ftl"):
                root = path.parent if path.parent.name.lower() in I18N_DIRS else path.parent.parent
                i18n_files.setdefault(root, []).append(path)

        if path.suffix not in SOURCE_EXTS or name in LOCKFILES or name == "package.json":
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        is_test = bool(re.search(r"(^|[/_.])tests?([/_.]|$)|_test\.|\.test\.|\.spec\.", rel(repo, path)))
        for m in MODEL_RE.finditer(text):
            model = m.group(1)
            family = next(MODEL_FAMILY[p] for p in sorted(MODEL_FAMILY, key=len, reverse=True) if model.startswith(p))
            s = add("models", family, model, "", path)
            s.extra["count"] = s.extra.get("count", 0) + 1
            if not is_test:
                s.extra["prod"] = True
        for m in ENDPOINT_RE.finditer(text):
            host = m.group(1).rstrip(".")
            if not re.search(r"(^|\.)(example|localhost|test|invalid)(\.|$)", host):
                add("endpoints", "api", host, m.group(2) or "", path)
        low = text.lower()
        for launcher, marks in LAUNCHER_MARKERS.items():
            if any(mk.lower() in low for mk in marks):
                add("integrations", "launcher", launcher, "", path)
        for source, marks in MOD_MARKERS.items():
            if any(mk.lower() in low for mk in marks):
                add("mods", "source", source, "", path)
        for m in ANTICHEAT_RE.finditer(text):
            add("anticheat", "guard", m.group(1), "", path)

    for root, files in i18n_files.items():
        keys: dict[str, set[str]] = {}
        for f in files:
            keys[rel(repo, f)] = _i18n_keys(f)
        add("i18n", "locale-set", rel(repo, root), f"{len(files)} files", root, keys={k: sorted(v) for k, v in keys.items()})
    return sorted(found.values(), key=lambda s: (s.area, s.kind, s.name))


def _i18n_keys(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8", errors="replace")
    if path.suffix == ".ftl":
        return set(re.findall(r"^([A-Za-z][\w-]*)\s*=", text, re.M))
    try:
        doc = json.loads(text)
    except json.JSONDecodeError:
        return set()
    out: set[str] = set()

    def flat(prefix: str, node) -> None:
        if isinstance(node, dict):
            for k, v in node.items():
                flat(f"{prefix}.{k}" if prefix else k, v)
        else:
            out.add(prefix)
    flat("", doc)
    return out


# ---------------------------------------------------------------- fetching

def fetched_name(url: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "_", url.split("://", 1)[-1]).strip("_")[:150]


class Fetcher:
    def __init__(self, out: Path, offline: bool = False, timeout: float = 15.0) -> None:
        self.dir = out / "fetched"
        self.offline = offline
        self.timeout = timeout
        self.misses: dict[str, str] = {}
        # Saved bodies read this run: file name -> bytes, listed in the report.
        self.used: dict[str, int] = {}

    def saved(self, url: str) -> Path:
        return self.dir / fetched_name(url)

    def text(self, url: str, headers: dict | None = None, secret: bool = False) -> str | None:
        saved = self.saved(url)
        if saved.is_file():
            self.used[saved.name] = saved.stat().st_size
            return saved.read_text(encoding="utf-8", errors="replace")
        if self.offline:
            self.misses[url] = "offline"
            return None
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                return resp.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as exc:
            if exc.code == 404:
                return None
            if not secret:
                self.misses[url] = f"HTTP {exc.code}"
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            if not secret:
                self.misses[url] = miss_reason(exc)
        return None

    def json(self, url: str, headers: dict | None = None, secret: bool = False):
        body = self.text(url, headers, secret)
        if body is None:
            return None
        try:
            return json.loads(body)
        except json.JSONDecodeError:
            if not secret:
                self.unusable(url)
            return None

    def unusable(self, url: str) -> None:
        """A body came back but held nothing usable: keep the source unchecked."""
        self.misses[url] = "saved body unparseable, truncated?" if self.saved(url).is_file() else "response unparseable"


def miss_reason(exc: BaseException) -> str:
    """A proxy that refuses CONNECT surfaces as URLError("Tunnel connection failed: 403 ...")."""
    reason = str(getattr(exc, "reason", exc))
    if "Tunnel connection failed" in reason or ("403" in reason and "proxy" in reason.lower()):
        return "proxy blocked (403)" if "403" in reason else "proxy blocked"
    return type(exc).__name__


def run_cmd(args: list[str], cwd: Path, timeout: int = 300) -> tuple[int, str]:
    try:
        p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 127, str(exc)


# ---------------------------------------------------------------- checks

def sparse_index_path(name: str) -> str:
    n = name.lower()
    if len(n) <= 2:
        return f"{len(n)}/{n}"
    if len(n) == 3:
        return f"3/{n[0]}/{n}"
    return f"{n[:2]}/{n[2:4]}/{n}"


def latest_crate(f: Fetcher, name: str) -> str | None:
    url = f"https://index.crates.io/{sparse_index_path(name)}"
    body = f.text(url)
    if body is not None and not body.strip():
        f.unusable(url)
        return None
    best = None
    for line in (body or "").splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            entry = None
        if not isinstance(entry, dict):
            # A cut-off index would otherwise give an older "latest".
            f.unusable(url)
            return None
        v = parse_version(entry.get("vers", ""))
        if v and not entry.get("yanked") and (best is None or v > best[0]):
            best = (v, entry["vers"])
    return best[1] if best else None


def check_versions(f: Fetcher, surfaces: list[Surface], kind: str, latest_of) -> list[Finding]:
    items = [s for s in surfaces if s.kind == kind]
    out: list[Finding] = []
    with cf.ThreadPoolExecutor(max_workers=8) as pool:
        latest = dict(zip((s.name for s in items), pool.map(lambda s: latest_of(f, s.name), items)))
    for s in items:
        newest = latest.get(s.name)
        lv = parse_version(newest or "")
        if not lv:
            continue
        reqs = [r for r in s.current.split(" | ") if r]
        bases = [b for b in (req_base(r) for r in reqs) if b]
        if not bases:
            continue
        base = min(bases)
        kind_ = bump_kind(base, lv)
        if kind_ == "none":
            continue
        caret_ok = all(matches_req(lv, r) for r in reqs) if kind == "cargo" else False
        if caret_ok:
            sev, action, note = "info", "auto", "inside the requirement; a lockfile refresh picks it up"
        elif kind_ == "major":
            sev, action, note = "medium", "card", "breaking upgrade; read the changelog and migrate"
        else:
            sev, action, note = "low", "auto", f"{kind_} bump"
        out.append(Finding("deps", f"{kind}:{s.name}", sev, action, s.current, newest, note, s.files[:3]))
    return out


def check_rust_advisories(f: Fetcher, repo: Path, cache: Path, surfaces: list[Surface]) -> list[Finding]:
    locks = [s.name for s in surfaces if s.kind == "cargo-lock"]
    out: list[Finding] = []
    for lock in locks:
        lock_path = repo / lock
        if shutil.which("cargo-audit") or _has_cargo_sub("audit"):
            code, stdout = run_cmd(["cargo", "audit", "--json", "-f", str(lock_path)], repo)
            try:
                data = json.loads(stdout)
            except json.JSONDecodeError:
                data = None
            if data:
                out += _from_cargo_audit(data, lock)
                continue
        db = cache / "advisory-db"
        if not (db / "crates").is_dir():
            if f.offline:
                f.misses["https://github.com/rustsec/advisory-db"] = "offline"
                continue
            code, _ = run_cmd(["git", "clone", "--depth", "1", "-q", "https://github.com/rustsec/advisory-db", str(db)], cache, 600)
            if code != 0:
                f.misses["https://github.com/rustsec/advisory-db"] = "git clone failed"
                continue
        out += match_rustsec(db, lock_path, lock)
    return mark_deny_ignores(dedupe(out), deny_ignores(repo))


def deny_ignores(repo: Path) -> dict[str, str]:
    """Advisory id -> reason from every deny.toml [advisories].ignore list."""
    out: dict[str, str] = {}
    for path in walk(repo):
        if path.name != "deny.toml":
            continue
        try:
            doc = tomllib.loads(path.read_text(encoding="utf-8"))
        except (tomllib.TOMLDecodeError, OSError):
            continue
        for item in (doc.get("advisories") or {}).get("ignore") or []:
            if isinstance(item, str):
                out.setdefault(item, "no reason given")
            elif isinstance(item, dict) and item.get("id"):
                out[item["id"]] = str(item.get("reason") or "no reason given").strip()
    return out


def mark_deny_ignores(findings: list[Finding], ignores: dict[str, str]) -> list[Finding]:
    """An advisory the repo already ignores is a watch item that carries its recorded reason."""
    for x in findings:
        adv = x.data.get("advisory")
        if adv in ignores:
            x.severity, x.action = "info", "watch"
            x.data["ignored"] = ignores[adv]
    return findings


def dedupe(findings: list[Finding]) -> list[Finding]:
    """One finding per (surface, current, note); files merged."""
    seen: dict[tuple[str, str, str], Finding] = {}
    for x in findings:
        key = (x.surface, x.current, x.note)
        if key in seen:
            seen[key].files = sorted(set(seen[key].files) | set(x.files))
        else:
            seen[key] = x
    return list(seen.values())


def _has_cargo_sub(sub: str) -> bool:
    return shutil.which("cargo") is not None and shutil.which(f"cargo-{sub}") is not None


def _from_cargo_audit(data: dict, lock: str) -> list[Finding]:
    out = []
    for v in (data.get("vulnerabilities") or {}).get("list", []):
        adv, pkg = v.get("advisory", {}), v.get("package", {})
        patched = (v.get("versions") or {}).get("patched", [])
        out.append(_advisory_finding(adv.get("id", "?"), pkg.get("name", "?"), pkg.get("version", ""),
                                     patched, adv.get("title", ""), adv.get("informational"), lock))
    return out


def _advisory_finding(adv_id, name, version, patched, title, informational, lock) -> Finding:
    cur = parse_version(version) or (0, 0, 0)
    compatible = [p for p in patched if req_base(p) and bump_kind(cur, req_base(p)) in ("patch", "minor", "none")]
    if informational:
        sev, action = "medium", "card"
    else:
        sev, action = "critical", "auto" if compatible else "card"
    note = f"{adv_id}: {title}".strip().rstrip(":")
    if informational:
        note += f" ({informational})"
    if not patched:
        note += "; no fixed release, replace or vendor-patch"
    return Finding("security", f"cargo:{name}", sev, action, version, ", ".join(patched) or "none", note, [lock],
                   {"advisory": adv_id})


def parse_rustsec(text: str) -> dict:
    m = re.search(r"```toml\s*\n(.*?)\n```", text, re.S)
    try:
        return tomllib.loads(m.group(1)) if m else tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return {}


def match_rustsec(db: Path, lock_path: Path, lock: str) -> list[Finding]:
    try:
        lock_doc = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return []
    pkgs: dict[str, set[str]] = {}
    for p in lock_doc.get("package", []):
        if str(p.get("source", "")).startswith("registry+"):
            pkgs.setdefault(p["name"], set()).add(p["version"])
    out = []
    for name, versions in pkgs.items():
        adv_dir = db / "crates" / name
        if not adv_dir.is_dir():
            continue
        for adv_file in sorted(adv_dir.glob("*.md")):
            doc = parse_rustsec(adv_file.read_text(encoding="utf-8", errors="replace"))
            adv, ver = doc.get("advisory", {}), doc.get("versions", {})
            if adv.get("withdrawn"):
                continue
            patched, unaffected = ver.get("patched", []), ver.get("unaffected", [])
            for v in versions:
                pv = parse_version(v)
                if not pv or any(matches_req(pv, r) for r in patched + unaffected):
                    continue
                head = re.search(r"^#\s+(.+)$", adv_file.read_text(encoding="utf-8", errors="replace").split("```", 2)[-1], re.M)
                title = adv.get("title") or (head.group(1).strip() if head else "")
                out.append(_advisory_finding(adv.get("id", adv_file.stem), name, v, patched, title,
                                             adv.get("informational"), lock))
    return out


def check_npm_audit(repo: Path, surfaces: list[Surface]) -> list[Finding]:
    if not shutil.which("npm"):
        return []
    out = []
    sev_map = {"critical": "critical", "high": "high", "moderate": "medium", "low": "low", "info": "info"}
    for s in surfaces:
        if s.kind != "npm-lock":
            continue
        cwd = (repo / s.name).parent
        _, stdout = run_cmd(["npm", "audit", "--json", "--package-lock-only"], cwd)
        try:
            data = json.loads(stdout or "{}")
        except json.JSONDecodeError:
            continue
        vulns = data.get("vulnerabilities") or {}
        for name, vuln in vulns.items():
            if not any(isinstance(v, dict) for v in vuln.get("via", [])):
                continue  # only a dependent of another advisory; counted there
            dependents = [n for n, o in vulns.items() if name in [v for v in o.get("via", []) if isinstance(v, str)]]
            fix = vuln.get("fixAvailable")
            major = isinstance(fix, dict) and fix.get("isSemVerMajor")
            titles = [v.get("title", "") for v in vuln.get("via", []) if isinstance(v, dict)]
            if dependents:
                titles.append(f"pulled in by {', '.join(dependents[:4])}")
            if major:
                titles.append(f"fix needs {fix.get('name')} {fix.get('version')} (semver major)")
            out.append(Finding("security", f"npm:{name}", sev_map.get(vuln.get("severity"), "medium"),
                               "auto" if fix and not major else "card", vuln.get("range", ""),
                               (f"{fix.get('name')} {fix.get('version')}" if isinstance(fix, dict) else ("fix available" if fix else "none")),
                               "; ".join(dict.fromkeys(t for t in titles if t))[:160] or "advisory", [s.name]))
    return dedupe(out)


def latest_npm(f: Fetcher, name: str) -> str | None:
    url = f"https://registry.npmjs.org/{urllib.parse.quote(name, safe='@')}/latest"
    data = f.json(url)
    version = data.get("version") if isinstance(data, dict) else None
    if data is not None and not isinstance(version, str):
        f.unusable(url)
        return None
    return version


def latest_pypi(f: Fetcher, name: str) -> str | None:
    url = f"https://pypi.org/pypi/{name}/json"
    data = f.json(url)
    info = data.get("info") if isinstance(data, dict) else None
    version = info.get("version") if isinstance(info, dict) else None
    if data is not None and not isinstance(version, str):
        f.unusable(url)
        return None
    return version


def check_actions(f: Fetcher, repo: Path, surfaces: list[Surface]) -> list[Finding]:
    out = []
    for s in surfaces:
        if s.kind != "action":
            continue
        url = f"https://github.com/{s.name}"
        saved = f.dir / fetched_name(url + ".tags")
        if saved.is_file():
            f.used[saved.name] = saved.stat().st_size
            tags_text = saved.read_text(encoding="utf-8", errors="replace")
        elif f.offline:
            f.misses[url] = "offline"
            continue
        else:
            code, tags_text = run_cmd(["git", "ls-remote", "--tags", "--refs", url], repo, 60)
            if code != 0:
                f.misses[url] = "git ls-remote failed"
                continue
        tags = [parse_version(t.rsplit("/", 1)[-1]) for t in tags_text.split()]
        tags = [t for t in tags if t]
        if not tags:
            if saved.is_file():
                f.misses[url] = "saved body unparseable, truncated?"
            continue
        newest = max(tags)
        for ref in s.current.split(" | "):
            if re.fullmatch(r"[0-9a-f]{40}", ref):
                out.append(Finding("ci", f"action:{s.name}", "info", "watch", ref[:12], f"v{newest[0]}.{newest[1]}.{newest[2]}",
                                   "pinned by SHA; refresh the pin when you review the release", s.files[:3]))
                continue
            cur = parse_version(ref)
            if cur and newest[0] > cur[0]:
                out.append(Finding("ci", f"action:{s.name}", "low", "card", ref, f"v{newest[0]}",
                                   "new major; check inputs and runner requirements", s.files[:3]))
    return out


def check_toolchains(f: Fetcher, surfaces: list[Surface]) -> list[Finding]:
    out = []
    rust = [s for s in surfaces if s.kind in ("rust-toolchain", "rust-msrv")]
    if rust:
        url = "https://static.rust-lang.org/dist/channel-rust-stable.toml"
        body = f.text(url)
        m = re.search(r'\[pkg\.rust\]\s*\nversion\s*=\s*"(\d+\.\d+\.\d+)', body or "")
        stable = m.group(1) if m else None
        if body is not None and not stable:
            f.unusable(url)
        for s in rust:
            cur = parse_version(s.current.split(" | ")[0]) if s.current else None
            if not stable or not cur:
                continue
            if s.kind == "rust-toolchain" and bump_kind(cur, parse_version(stable)) != "none":
                out.append(Finding("toolchain", "rust toolchain pin", "low", "card", s.current, stable,
                                   "pinned toolchain behind stable; new lints may fire", s.files))
            elif s.kind == "rust-msrv" and parse_version(stable)[1] - cur[1] > 12:
                out.append(Finding("toolchain", "rust-version (MSRV)", "info", "watch", s.current, stable,
                                   "floor is far behind stable", s.files[:3]))
    node = [s for s in surfaces if s.kind in ("node-engine", "node-pin")]
    if node:
        url = "https://nodejs.org/dist/index.json"
        index = f.json(url)
        rows = [r for r in index if isinstance(r, dict)] if isinstance(index, list) else []
        lts = next((r for r in rows if r.get("lts") and parse_version(str(r.get("version", "")))), None)
        if index is not None and not lts:
            f.unusable(url)
        for s in node:
            cur = req_base(s.current)
            if not lts or not cur:
                continue
            latest = parse_version(lts["version"])
            if s.kind == "node-pin" and latest[0] > cur[0]:
                out.append(Finding("toolchain", "node pin", "low", "card", s.current, lts["version"],
                                   f"newer LTS line ({lts['lts']})", s.files))
            elif s.kind == "node-engine" and latest[0] - cur[0] >= 2:
                out.append(Finding("toolchain", "node engines", "info", "watch", s.current, lts["version"],
                                   "engines floor is two or more LTS lines behind", s.files))
    return out


def wow_builds(f: Fetcher, products: dict[str, str]) -> dict[str, str]:
    """Live client builds per product from Blizzard's version service."""
    out = {}
    for product in products:
        url = f"https://us.version.battle.net/v2/products/{product}/versions"
        body = f.text(url)
        for line in (body or "").splitlines():
            cols = line.split("|")
            if cols and cols[0] == "us" and len(cols) > 5:
                out[product] = cols[5]
                break
        else:
            if body is not None:
                f.unusable(url)
    return out


def check_wow(f: Fetcher, surfaces: list[Surface], products: dict[str, str]) -> list[Finding]:
    tocs = [s for s in surfaces if s.kind == "wow-toc"]
    if not tocs:
        return []
    builds = wow_builds(f, products)
    out = []
    for s in tocs:
        have = {int(n) for n in s.current.split(",") if n.strip().isdigit()}
        for product, build in sorted(builds.items()):
            iface = wow_interface(build)
            if iface and iface not in have:
                out.append(Finding("addons", f"wow:{products[product]}", "high", "auto", ",".join(str(n) for n in sorted(have, reverse=True)),
                                   str(iface), f"client {build} not in ## Interface; the game flags the addon out of date",
                                   [s.name], {"toc": s.name, "interface": iface}))
    return out


def check_minecraft(f: Fetcher, surfaces: list[Surface]) -> list[Finding]:
    mc = [s for s in surfaces if s.kind.startswith("minecraft") or (s.kind == "launcher" and s.name == "Minecraft")]
    if not mc:
        return []
    url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
    data = f.json(url)
    latest = data.get("latest") if isinstance(data, dict) else None
    release = latest.get("release") if isinstance(latest, dict) else None
    if not isinstance(release, str) or not release:
        if data is not None:
            f.unusable(url)
        return []
    out = []
    for s in mc:
        if s.kind == "minecraft-mod" and s.current and release not in s.current:
            out.append(Finding("addons", f"minecraft:{s.name}", "medium", "card", s.current, release,
                               "mod's minecraft range may not cover the latest release", s.files))
        elif s.kind == "launcher":
            out.append(Finding("integrations", "Minecraft", "info", "watch", "", release,
                               "latest release; recheck save and log readers when the data version changes", s.files[:3]))
    return out


def check_models(f: Fetcher, surfaces: list[Surface]) -> list[Finding]:
    models = [s for s in surfaces if s.area == "models"]
    if not models:
        return []
    families = sorted({s.kind for s in models})
    catalog: dict[str, list[tuple[str, int]]] = {}
    source: dict[str, str] = {}
    for fam in families:
        env, url, hdr = OFFICIAL_MODEL_LISTS.get(fam, (None, None, None))
        if env and os.environ.get(env):
            data = f.json(url, hdr(os.environ[env]), secret=True)
            if isinstance(data, dict) and isinstance(data.get("data"), list):
                catalog[fam] = [(m.get("id", ""), int(m.get("created", 0) or 0)) for m in data["data"]]
                source[fam] = "official"
    missing = [fam for fam in families if fam not in catalog and fam in OPENROUTER_PREFIX]
    if missing:
        url = "https://openrouter.ai/api/v1/models"
        data = f.json(url)
        if data is not None and not (isinstance(data, dict) and isinstance(data.get("data"), list)):
            f.unusable(url)
        rows = data.get("data") if isinstance(data, dict) and isinstance(data.get("data"), list) else []
        for m in rows:
            mid = m.get("id", "")
            for fam in missing:
                pre = OPENROUTER_PREFIX[fam]
                if mid.startswith(pre):
                    catalog.setdefault(fam, []).append((mid[len(pre):].split(":")[0], int(m.get("created", 0) or 0)))
                    source[fam] = "openrouter"
    out = []
    for fam in families:
        ids = catalog.get(fam)
        if not ids:
            continue
        known = {i.replace(".", "-") for i, _ in ids}
        unknown = sorted(
            s.name for s in models
            if s.kind == fam and s.extra.get("prod") and is_hosted_id(s.name)
            and s.name.replace(".", "-") not in known
        )
        if unknown:
            sev = "medium" if source[fam] == "official" else "low"
            files = sorted({fl for s in models if s.name in unknown for fl in s.files})[:4]
            out.append(Finding("models", f"{fam}:unlisted", sev, "manual", f"{len(unknown)} ids", "",
                               f"not in the {source[fam]} list (retired, renamed or alias?): " + ", ".join(unknown[:8]),
                               files, {"ids": unknown}))
        used = {s.name for s in models if s.kind == fam}
        fresh = [i for i, _ in sorted(ids, key=lambda x: -x[1]) if i not in used]
        fresh = list(dict.fromkeys(fresh))[:3]
        if fresh:
            out.append(Finding("models", f"{fam}:newest", "info", "watch", "", ", ".join(fresh),
                               f"newest in the {source[fam]} list that the code never names", []))
    return out


def is_hosted_id(name: str) -> bool:
    """Hosted API ids carry a version digit; local weights carry quant tags."""
    return bool(re.search(r"\d", name)) and not re.search(
        r"(?i)gguf|q\d(_k)?|\.tar|\.zip|-bin-|abliterat|uncensored|awq|gptq|exl2|mlx|latest|test|example|fake|mock", name)


def check_watch(f: Fetcher, surfaces: list[Surface], probe_mods: bool) -> list[Finding]:
    out = []
    hosts: dict[str, Surface] = {}
    for s in surfaces:
        if s.area == "endpoints":
            hosts[s.name] = s
        elif s.area == "integrations" and s.name != "Minecraft":
            out.append(Finding("integrations", s.name, "info", "watch", "", "",
                               "recheck its data paths and formats after its next update", s.files[:3]))
        elif s.area == "mods":
            note = "mod source the app talks to; recheck API version and terms"
            if probe_mods and s.name in MOD_SOURCES and f.text(MOD_SOURCES[s.name]) is None:
                note += "; probe failed"
            out.append(Finding("mods", s.name, "info", "watch", "", "", note, s.files[:3]))
        elif s.area == "anticheat":
            out.append(Finding("anticheat", s.name, "info", "guarded", "", "",
                               "read-only guard; never auto-edited, changes may only add protection", s.files[:4]))
        elif s.area == "i18n":
            keys = s.extra.get("keys", {})
            base = next((k for k in keys if re.search(r"(^|/)en([-_]US)?(\.|/)", k)), None)
            if base:
                want = set(keys[base])
                for k, have in keys.items():
                    gap = len(want - set(have))
                    if k != base and gap:
                        out.append(Finding("i18n", k, "low", "card", f"{len(have)} keys", f"{len(want)} keys",
                                           f"{gap} strings missing versus {base}", [k]))
    for host, s in sorted(hosts.items()):
        versions = sorted(v for v in s.current.split(" | ") if v)
        out.append(Finding("endpoints", host, "info", "watch", ", ".join(versions), "",
                           "API the app calls; check its changelog for deprecations", s.files[:2]))
    return out


# ---------------------------------------------------------------- report

def rank(findings: list[Finding]) -> list[Finding]:
    return sorted(findings, key=lambda x: (SEVERITY_ORDER.get(x.severity, 9), x.area, x.surface))


def render(findings: list[Finding], surfaces: list[Surface], misses: dict[str, str], repo: Path, limit: int,
           used: dict[str, int] | None = None) -> str:
    ranked = rank(findings)
    actionable = [x for x in ranked if x.action in ("auto", "card", "manual") and x.severity != "info"]
    lines = [f"# Updater report: {repo.name}", ""]
    counts: dict[str, int] = {}
    for s in surfaces:
        counts[s.area] = counts.get(s.area, 0) + 1
    lines.append("Surfaces: " + ", ".join(f"{a} {n}" for a, n in sorted(counts.items())))
    by_sev: dict[str, int] = {}
    for x in actionable:
        by_sev[x.severity] = by_sev.get(x.severity, 0) + 1
    lines.append("Findings: " + (", ".join(f"{s} {by_sev[s]}" for s in SEVERITY_ORDER if s in by_sev) or "none"))
    auto_areas: dict[str, int] = {}
    for x in ranked:
        if x.action == "auto":
            auto_areas[bump_area(x)] = auto_areas.get(bump_area(x), 0) + 1
    if auto_areas:
        lines.append("Safe bumps: " + ", ".join(f"{a} {n} (bump --area {a})" for a, n in sorted(auto_areas.items())))
    lines += ["", "| sev | action | surface | current | latest | note |", "|---|---|---|---|---|---|"]
    for x in actionable[:limit]:
        lines.append(f"| {x.severity} | {x.action} | {x.surface} | {_cut(x.current)} | {_cut(x.latest)} | {_cut(x.note, 90)} |")
    if len(actionable) > limit:
        lines.append(f"| … | | {len(actionable) - limit} more in findings.json | | | |")
    watch = [x for x in ranked if x.action in ("watch", "guarded") or x.severity == "info"]
    compat = [x for x in watch if x.action == "auto"]
    if compat:
        lines += ["", f"Lockfile-only: {len(compat)} deps have newer compatible releases (a refresh picks them up)."]
    other = [x for x in watch if x.action != "auto" and not x.data.get("ignored")]
    if other:
        lines += ["", "Watch list (no action now):"]
        groups: dict[str, list[Finding]] = {}
        for x in other:
            groups.setdefault(x.area, []).append(x)
        for area, xs in sorted(groups.items()):
            shown = "; ".join(f"{x.surface}{' ' + _cut(x.latest, 40) if x.latest else ''}" for x in xs[:6])
            more = f" (+{len(xs) - 6})" if len(xs) > 6 else ""
            lines.append(f"- {area}: {shown}{more}")
    ignored = [x for x in ranked if x.data.get("ignored")]
    if ignored:
        lines += ["", "Ignored advisories (deny.toml; name each in the report with its reason):"]
        lines += [f"- {x.data['advisory']} {x.surface} {x.current}: {_cut(x.data['ignored'], 120)}" for x in ignored]
    if misses:
        lines += ["", "Unchecked sources (fetch elsewhere, save under fetched/<file>, rerun):"]
        for url, why in sorted(misses.items()):
            lines.append(f"- {url} ({why}) -> {fetched_name(url + ('.tags' if url.count('/') == 4 and 'github.com' in url else ''))}")
    if used:
        lines += ["", "Saved bodies used (fetched/):"]
        lines += [f"- {name} ({size} bytes)" for name, size in sorted(used.items())]
    return "\n".join(lines) + "\n"


def _cut(text: str, n: int = 40) -> str:
    text = str(text).replace("|", "/").replace("\n", " ")
    return text if len(text) <= n else text[: n - 1] + "…"


def bump_area(x: Finding) -> str:
    if x.surface.startswith("wow:"):
        return "wow"
    return x.surface.split(":", 1)[0]


# ---------------------------------------------------------------- bump

def lock_packages(lock: Path) -> set[tuple[str, str]]:
    try:
        doc = tomllib.loads(lock.read_text(encoding="utf-8"))
    except (tomllib.TOMLDecodeError, OSError):
        return set()
    return {(p.get("name", ""), p.get("version", "")) for p in doc.get("package", [])}


def _major(version: str) -> str:
    v = parse_version(version) or (0, 0, 0)
    return f"{v[0]}" if v[0] else f"0.{v[1]}"


def lock_review(before: set[tuple[str, str]], after: set[tuple[str, str]]) -> list[str]:
    """New crates and new duplicate majors a lock refresh pulled in."""
    old_names = {n for n, _ in before}
    out = [f"new crate {n} {v}" for n, v in sorted(after - before) if n not in old_names]

    def majors(pkgs):
        m: dict[str, set[str]] = {}
        for n, v in pkgs:
            m.setdefault(n, set()).add(_major(v))
        return m
    was = majors(before)
    for name, ms in sorted(majors(after).items()):
        if len(ms) > 1 and ms != was.get(name, set()) and len(ms) > len(was.get(name, set())):
            vers = ", ".join(sorted(v for n, v in after if n == name))
            out.append(f"duplicate majors {name} ({vers}); hold the crate that pulls the new one in"
                       f" (cargo update -p <crate>@<new> --precise <old>) and card it")
    return out


def bump(repo: Path, out: Path, area: str) -> list[str]:
    findings = [Finding(**d) for d in json.loads((out / "findings.json").read_text(encoding="utf-8"))]
    todo = [x for x in findings if x.action == "auto" and bump_area(x) == area]
    changed: list[str] = []
    if area == "cargo":
        for lock in sorted({fl for x in todo for fl in x.files if fl.endswith("Cargo.lock")} or {"Cargo.lock"}):
            cwd = (repo / lock).parent
            before = lock_packages(repo / lock)
            code, _ = run_cmd(["cargo", "update"], cwd, 900)
            if code == 0:
                changed.append(lock)
                for line in lock_review(before, lock_packages(repo / lock)):
                    print(f"review {lock}: {line}")
    elif area == "npm":
        for x in todo:
            if x.area != "deps":
                continue
            name = x.surface.split(":", 1)[1]
            for manifest in x.files:
                p = repo / manifest
                doc = json.loads(p.read_text(encoding="utf-8"))
                for key in ("dependencies", "devDependencies", "optionalDependencies"):
                    spec = (doc.get(key) or {}).get(name)
                    if isinstance(spec, str):
                        m = re.match(r"^([\^~=]?)v?", spec)
                        doc[key][name] = f"{m.group(1)}{x.latest}"
                text = json.dumps(doc, indent=2, ensure_ascii=False) + "\n"
                if text != p.read_text(encoding="utf-8"):
                    p.write_text(text, encoding="utf-8")
                    changed.append(manifest)
        for manifest in sorted(set(changed) | {fl for x in todo if x.area == "security" for fl in x.files}):
            cwd = (repo / manifest).parent
            if shutil.which("npm"):
                run_cmd(["npm", "install", "--package-lock-only", "--ignore-scripts"], cwd, 900)
                if any(x.area == "security" for x in todo):
                    run_cmd(["npm", "audit", "fix", "--package-lock-only", "--ignore-scripts"], cwd, 900)
    elif area == "pypi":
        for x in todo:
            name = x.surface.split(":", 1)[1]
            for manifest in x.files:
                p = repo / manifest
                text = p.read_text(encoding="utf-8")
                new = re.sub(rf"(?im)^(\s*{re.escape(name)}(\[[^\]]*\])?\s*==\s*)[\w.]+", rf"\g<1>{x.latest}", text)
                if new != text:
                    p.write_text(new, encoding="utf-8")
                    changed.append(manifest)
    elif area == "wow":
        add: dict[str, set[int]] = {}
        for x in todo:
            add.setdefault(x.data["toc"], set()).add(int(x.data["interface"]))
        for toc, nums in add.items():
            p = repo / toc
            text = p.read_text(encoding="utf-8")
            new = set_toc_interfaces(text, nums)
            if new != text:
                p.write_text(new, encoding="utf-8")
                changed.append(toc)
    else:
        raise SystemExit(f"no safe bump for area {area!r}; card it instead")
    return sorted(set(changed))


def set_toc_interfaces(text: str, extra: set[int]) -> str:
    def repl(m: re.Match) -> str:
        nums = {int(n) for n in re.findall(r"\d+", m.group(2))} | extra
        return m.group(1) + ", ".join(str(n) for n in sorted(nums, reverse=True))
    return re.sub(r"^(##\s*Interface:\s*)([\d,\s]+?)$", repl, text, count=1, flags=re.M)


# ---------------------------------------------------------------- main

def default_out(repo: Path) -> Path:
    base = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    return base / "vstack-updater" / repo.resolve().name


def run(repo: Path, out: Path, offline: bool, limit: int, probe_mods: bool, wow_ptr: bool) -> str:
    out.mkdir(parents=True, exist_ok=True)
    f = Fetcher(out, offline)
    surfaces = inventory(repo)
    (out / "inventory.json").write_text(json.dumps([asdict(s) for s in surfaces], indent=1), encoding="utf-8")
    products = dict(WOW_PRODUCTS)
    if wow_ptr:
        products.update({"wowt": "retail ptr", "wowxptr": "retail xptr", "wow_classic_ptr": "classic ptr"})
    findings: list[Finding] = []
    findings += check_versions(f, surfaces, "cargo", latest_crate)
    findings += check_versions(f, surfaces, "npm", latest_npm)
    findings += check_versions(f, surfaces, "pypi", latest_pypi)
    findings += check_rust_advisories(f, repo, out, surfaces)
    findings += check_npm_audit(repo, surfaces)
    findings += check_actions(f, repo, surfaces)
    findings += check_toolchains(f, surfaces)
    findings += check_wow(f, surfaces, products)
    findings += check_minecraft(f, surfaces)
    findings += check_models(f, surfaces)
    findings += check_watch(f, surfaces, probe_mods)
    findings = rank(findings)
    (out / "findings.json").write_text(json.dumps([asdict(x) for x in findings], indent=1), encoding="utf-8")
    report = render(findings, surfaces, f.misses, repo, limit, f.used)
    (out / "report.md").write_text(report, encoding="utf-8")
    return report


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", nargs="?", default="run", choices=["run", "bump"])
    ap.add_argument("--repo", default=".", type=Path)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--area", help="bump: cargo, npm, pypi or wow")
    ap.add_argument("--offline", action="store_true", help="use only fetched/ files")
    ap.add_argument("--limit", type=int, default=40, help="rows in report.md")
    ap.add_argument("--probe-mods", action="store_true", help="ping mod source APIs")
    ap.add_argument("--wow-ptr", action="store_true", help="also check WoW test realms")
    args = ap.parse_args(argv)
    repo = args.repo.resolve()
    out = (args.out or default_out(repo)).resolve()
    if args.command == "bump":
        if not args.area:
            ap.error("bump needs --area")
        for path in bump(repo, out, args.area):
            print(path)
        return 0
    sys.stdout.write(run(repo, out, args.offline, args.limit, args.probe_mods, args.wow_ptr))
    print(f"\n(full data: {out}/findings.json)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
