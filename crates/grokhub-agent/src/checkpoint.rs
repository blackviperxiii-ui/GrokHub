//! Brain 2 checkpoints: what a run must still show before it may end, beyond
//! its own checklist ([`crate::drive`]). Each change (a file write or edit, a
//! shell command that isn't read-only, a desktop action) waits for the check
//! that fits it ([`Needs`]): an edit to source code for a command that runs
//! (a test, build or lint run, or any other command that is neither a look
//! nor a change, such as running the changed file), any other file for a run
//! or a re-read of that same path, a shell change for a run or a read-only
//! look, and a desktop action for a run or a screenshot. A re-read, `git
//! status`, `git diff`, `cat` or `sed -n` never checks code. Each command of
//! a shell line is read by its program and subcommand, with quoted text,
//! comments and here-document bodies left out: a test word in a commit
//! message or a grep pattern runs nothing. Every change waits on its own, by
//! its full path. A careful or critical task, repair or research run
//! must cover each item of its Brief's done list in the checklist, and a
//! critical system change needs a backup. Only a run with a Brief (a chat or
//! background turn) owes these. Nothing here caps steps; the drive's stall
//! count ends a run that can't make progress, naming what is left.
//!
//! Brain 5: a careful repair or research run also owes a findings card, and
//! a long task in a progress style gets a short note at each phase boundary
//! (a checklist item closing), rate-limited by [`PROGRESS_GAP`].

use std::sync::Mutex;

use serde_json::Value;

use grokhub_core::findings::{
    is_impossible_one_tap, Finding, Findings, Fix, Severity, MAX_FINDINGS,
};
use grokhub_core::talk::TalkStyle;

use crate::brain::{Brief, Care, Intent};
use crate::lru_map::LruMap;
use crate::session_tools::TodoItem;

// File shadow checkpoints live in `file_checkpoint` (this module is Brain 2
// verify gaps). Re-export so call sites can use `checkpoint::before_write`.
pub use crate::file_checkpoint::before_write;

/// Task tool calls between mid-run checkpoints.
pub const CHECKPOINT_EVERY: usize = 8;
/// Task tool calls at least between two progress notes (and before the first).
pub const PROGRESS_GAP: usize = 3;
/// Items a progress note or the unchecked list names before "and N more".
const PROGRESS_ITEMS: usize = 3;
/// Longest item text in a progress note or fallback card, in characters.
const ITEM_CHARS: usize = 60;

// Word lists are lowercase; commands are compared lowercased (PowerShell names too).

/// A command's program or one of its next two plain words that makes it a
/// check run: tests, builds, linters (`cargo test`, `npm run lint`, `pytest`).
const RUN_WORDS: &[&str] = &[
    "test",
    "tests",
    "nextest",
    "pytest",
    "jest",
    "vitest",
    "check",
    "build",
    "clippy",
    "lint",
    "eslint",
    "tsc",
    "typecheck",
    "mypy",
    "ruff",
    "verify",
];
/// A command's program, its subcommand or a `--version` that makes it a
/// read-only look: it checks a shell change, never code.
const LOOK_WORDS: &[&str] = &[
    "status",
    "is-active",
    "is-enabled",
    "--version",
    "diff",
    "test-path",
    "get-service",
    "show",
    "view",
    "list",
    "info",
];
/// Programs that only show something (a file, a search, a listing, a
/// value): a look, never a run, so they never check code. `sed`, `perl` and
/// `ruby` with `-i` edit in place, and `find` with `-delete` or `-exec`
/// acts; [`command_kind`] tells those apart. `command` is here for
/// `command -v`, the only way it stays first.
const VIEWERS: &[&str] = &[
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "nl",
    "wc",
    "sed",
    "awk",
    "gawk",
    "mawk",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "ack",
    "find",
    "fd",
    "tree",
    "ls",
    "dir",
    "file",
    "stat",
    "du",
    "df",
    "echo",
    "printf",
    "which",
    "whereis",
    "where",
    "type",
    "command",
    "realpath",
    "readlink",
    "basename",
    "dirname",
    "pwd",
    "whoami",
    "id",
    "uname",
    "printenv",
    "ps",
    "uptime",
    "free",
    "jq",
    "yq",
    "bat",
    "xxd",
    "hexdump",
    "od",
    "strings",
    "column",
    "tac",
    "rev",
    "cut",
    "tr",
    "comm",
    "cmp",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "journalctl",
    "dmesg",
    "get-content",
    "gc",
    "select-string",
    "sls",
    "findstr",
    "get-childitem",
    "gci",
    "get-item",
    "get-itemproperty",
    "get-location",
    "resolve-path",
    "get-command",
    "get-process",
    "get-filehash",
    "get-date",
    "measure-object",
    "select-object",
    "where-object",
    "sort-object",
    "format-table",
    "format-list",
    "out-string",
    "out-host",
    "out-null",
    "write-host",
    "write-output",
];
/// Commands that set up the next one rather than run anything: a change of
/// folder, a variable, a pause, shell syntax. They neither change nor check.
const SETUP: &[&str] = &[
    "cd",
    "pushd",
    "popd",
    "chdir",
    "set-location",
    "export",
    "set",
    "unset",
    "alias",
    "source",
    ".",
    "true",
    "false",
    ":",
    "sleep",
    "start-sleep",
    "clear",
    "cls",
    "exit",
    "wait",
    "}",
    "fi",
    "done",
    "esac",
    "for",
    "case",
    "local",
    "declare",
    "trap",
    "ulimit",
    "umask",
];
/// Words that run the command after them, with their flags that take a
/// value: `sudo -u app cargo test` runs `cargo test`.
const WRAPPERS: &[(&str, &[&str])] = &[
    (
        "sudo",
        &["-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-U"],
    ),
    ("doas", &["-u", "-C"]),
    ("env", &["-u", "-C", "-S"]),
    ("time", &[]),
    ("nohup", &[]),
    ("exec", &[]),
    ("builtin", &[]),
    ("command", &[]),
    ("nice", &["-n"]),
    ("ionice", &["-c", "-n", "-p"]),
    ("timeout", &["-s", "-k"]),
    ("stdbuf", &["-i", "-o", "-e"]),
    ("xargs", &["-a", "-d", "-E", "-I", "-L", "-n", "-P", "-s"]),
    ("npx", &["-p", "--package", "-c"]),
    ("bunx", &["-p", "--package"]),
    ("pnpx", &[]),
    ("uvx", &["--from", "--with", "--python"]),
    ("{", &[]),
    ("!", &[]),
    ("if", &[]),
    ("then", &[]),
    ("else", &[]),
    ("elif", &[]),
    ("do", &[]),
    ("while", &[]),
    ("until", &[]),
    ("foreach", &[]),
    ("foreach-object", &[]),
    ("%", &[]),
];
/// Two words that run the command after them: `uv run pytest`,
/// `python -m pytest`.
const RUNNERS: &[(&str, &str)] = &[
    ("uv", "run"),
    ("poetry", "run"),
    ("pipenv", "run"),
    ("pdm", "run"),
    ("hatch", "run"),
    ("rye", "run"),
    ("bundle", "exec"),
    ("npm", "exec"),
    ("pnpm", "exec"),
    ("pnpm", "dlx"),
    ("yarn", "dlx"),
    ("yarn", "exec"),
    ("python", "-m"),
    ("python3", "-m"),
    ("py", "-m"),
];
/// Source code extensions: an edit to one needs a command that runs. Shell
/// scripts, docs and config files stay checkable by a re-read.
const CODE_EXTS: &[&str] = &[
    "rs", "py", "js", "jsx", "mjs", "cjs", "ts", "tsx", "go", "c", "h", "cc", "cpp", "hpp", "cs",
    "java", "kt", "swift", "rb", "php", "vue", "svelte",
];
/// Words in a shell command that save the state before a change.
const BACKUP_WORDS: &[&str] = &[
    "backup",
    "timeshift",
    "snapper",
    "snapshot",
    "checkpoint-computer",
    "etckeeper",
];
/// A copy to a `.bak` or `.orig` name is a backup too.
const COPY_WORDS: &[&str] = &["cp", "rsync", "tar", "copy-item", "copy"];
/// Words in a shell command, or path parts, that touch the system.
const SYSTEM_WORDS: &[&str] = &[
    "sudo",
    "systemctl",
    "apt",
    "apt-get",
    "pacman",
    "dnf",
    "yum",
    "flatpak",
    "snap",
    "modprobe",
    "grub-mkconfig",
    "update-grub",
    "ufw",
    "iptables",
    "chmod",
    "chown",
    "mount",
    "fstab",
    "reg",
    "sc.exe",
    "set-service",
];
const SYSTEM_PATHS: &[&str] = &[
    "/etc/",
    "/boot/",
    "/usr/",
    "/lib/",
    "/var/lib/",
    "c:\\windows",
    "c:/windows",
];
/// A command's program or subcommand that makes it change files, packages,
/// services or settings (`rm`, `pip install`, `systemctl restart`). A system
/// word alone (`systemctl status`) is not a change, and neither is one an
/// argument names (`cargo test update`).
const MUTATING_HEADS: &[&str] = &[
    "rm",
    "mv",
    "cp",
    "mkdir",
    "rmdir",
    "touch",
    "tee",
    "ln",
    "install",
    "uninstall",
    "remove",
    "add",
    "commit",
    "push",
    "kill",
    "pkill",
    "remove-item",
    "set-content",
    "new-item",
    "move-item",
    "copy-item",
    "rename-item",
    "add-content",
    "clear-content",
    "out-file",
    "set-itemproperty",
    "new-itemproperty",
    "restart",
    "start",
    "stop",
    "enable",
    "disable",
    "mask",
    "unmask",
    "reload",
    "purge",
    "upgrade",
    "update",
    "chmod",
    "chown",
    "mount",
    "umount",
    "modprobe",
    "update-grub",
    "grub-mkconfig",
    "allow",
    "deny",
    "set-service",
];
const DESKTOP_ACTIONS: &[&str] = &["click", "move", "drag", "scroll", "type", "key"];
const STOP_WORDS: &[&str] = &["the", "is", "it", "a", "an", "in", "of", "and", "to"];

/// What proves one change was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// Source code: a test, build or lint run, or any command that runs.
    Run,
    /// Any other file: a run, or a re-read of the same path.
    Reread,
    /// A shell change: a run, or any read-only look.
    Look,
    /// A desktop action: a run, or a screenshot.
    Screenshot,
}

impl Needs {
    /// `step` checks the change to `what` that needs this.
    fn checked_by(self, what: &str, step: &Step) -> bool {
        match (self, step) {
            (_, Step::Run) | (Needs::Run, Step::Exec) => true,
            (Needs::Reread, Step::Reread(path)) => same_path(what, path),
            (Needs::Look, Step::Look) | (Needs::Screenshot, Step::Screenshot) => true,
            _ => false,
        }
    }
}

/// What one finished tool call was, for the checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Changed this (a file path, a command, "the screen").
    Change {
        what: String,
        system: bool,
        needs: Needs,
    },
    /// A test, build or lint run: it checks every change.
    Run,
    /// A re-read of this path.
    Reread(String),
    Screenshot,
    /// A read-only look (`git status`, `ls`, `cat`).
    Look,
    /// Any other command that ran: neither a look nor a change.
    Exec,
    Backup,
    Other,
}

fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '"' | '\''))
        .filter(|w| !w.is_empty())
        .collect()
}

fn path_arg(args: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string()
}

fn system_path(p: &str) -> bool {
    let p = p.to_ascii_lowercase();
    SYSTEM_PATHS.iter().any(|s| p.starts_with(s))
}

/// A source code file, by its extension.
fn is_code(path: &str) -> bool {
    let name = path.replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or("");
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| CODE_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn classify(name: &str, args: &Value) -> Vec<Step> {
    let one = |step| vec![step];
    match name {
        "write" | "search_replace" => {
            let path = path_arg(args, &["path", "file_path", "target_file"]);
            let lower = path.to_ascii_lowercase();
            // A copy written next to a system file (`/etc/fstab.bak`) is a backup;
            // other workspace files are ordinary changes, whatever their name.
            if (lower.ends_with(".bak") || lower.ends_with(".orig")) && system_path(&lower) {
                return one(Step::Backup);
            }
            one(Step::Change {
                system: system_path(&path),
                needs: if is_code(&path) {
                    Needs::Run
                } else {
                    Needs::Reread
                },
                what: path,
            })
        }
        "read_file" => one(Step::Reread(path_arg(
            args,
            &["target_file", "path", "file_path"],
        ))),
        "screenshot" => one(Step::Screenshot),
        n if DESKTOP_ACTIONS.contains(&n) => one(Step::Change {
            what: DESKTOP_WHAT.into(),
            system: false,
            needs: Needs::Screenshot,
        }),
        "run_terminal_command" => shell_steps(
            args.get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim(),
        ),
        _ => one(Step::Other),
    }
}

/// What one command of a shell line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Sets up the next command (`cd`, `export`): neither changes nor checks.
    Setup,
    /// Only shows something.
    Look,
    Change,
    /// A test, build or lint run.
    Run,
    /// Any other program that ran.
    Exec,
}

/// A shell line, in order: a backup it takes, then each of its commands
/// (split at `&&`, `||`, `;`, `|`, `&` and line ends) as the step it is, and a
/// redirect that writes a file as a change where it starts. Quoted text, a
/// comment and a here-document's body are data, so a word in them is never
/// read as a command: `git commit -m "fix test"` runs no test.
fn shell_steps(cmd: &str) -> Vec<Step> {
    let code = crate::shell_redirect::code(cmd);
    let lower = code.to_ascii_lowercase();
    let ws = words(&lower);
    let writes: Vec<(usize, String)> = crate::shell_redirect::redirects(cmd)
        .into_iter()
        .filter_map(|r| r.file.map(|file| (r.at, file)))
        .collect();
    let mut steps = Vec::new();
    let original = cmd.to_ascii_lowercase();
    if ws.iter().any(|w| BACKUP_WORDS.contains(w))
        || (ws.iter().any(|w| COPY_WORDS.contains(w))
            && (original.contains(".bak") || original.contains(".orig")))
    {
        steps.push(Step::Backup);
    }
    let system = ws
        .iter()
        .any(|w| SYSTEM_WORDS.contains(w) || system_path(w))
        || writes.iter().any(|(_, file)| system_path(file));
    let mut kinds: Vec<(usize, Kind, String)> = Vec::new();
    let mut before = Kind::Setup;
    for (at, piped) in commands(&code) {
        let read = command_kind(&cmd[at.clone()], &code[at.clone()]);
        // `echo hi > out.txt` writes its output, so nobody sees it, and
        // `npm install | tail` shows the install's own lines, not its result.
        let unseen = read.kind == Kind::Look
            && (writes.iter().any(|(w, _)| at.contains(w)) || (piped && before == Kind::Change));
        if read.builds {
            kinds.push((at.start, Kind::Exec, String::new()));
        }
        before = read.kind;
        kinds.push((
            at.start,
            if unseen { Kind::Setup } else { read.kind },
            read.program,
        ));
    }
    // `sudo cp /etc/fstab /etc/fstab.bak` is the backup, not a change to check.
    let backup_only = !steps.is_empty()
        && writes.is_empty()
        && kinds.iter().all(|(_, kind, program)| {
            *kind != Kind::Change || COPY_WORDS.contains(&program.as_str())
        });
    kinds.extend(
        writes
            .iter()
            .map(|(at, _)| (*at, Kind::Change, String::new())),
    );
    kinds.sort_by_key(|(at, _, _)| *at);
    // In order, so `restart && is-active` checks itself, `edit && cargo test`
    // checks everything and `make > build.log` ran before it wrote the log.
    for (_, kind, _) in kinds {
        match kind {
            Kind::Change if !backup_only => steps.push(Step::Change {
                what: short(cmd),
                system,
                needs: Needs::Look,
            }),
            Kind::Look => steps.push(Step::Look),
            Kind::Run => steps.push(Step::Run),
            Kind::Exec => steps.push(Step::Exec),
            Kind::Change | Kind::Setup => {}
        }
    }
    steps
}

/// Each command of a shell line's [`crate::shell_redirect::code`]: its byte
/// range, and whether a lone `|` feeds it the output of the command before.
/// A command ends at `&&`, `||`, `;`, `|`, `&`, a line end and a parenthesis
/// (a subshell or `$(…)` runs its own commands). The rest of the word after
/// a closing parenthesis (`$(pwd)/src`, `(Get-Content f)[0..9]`) is the outer
/// command's data, not a new command.
fn commands(code: &str) -> Vec<(std::ops::Range<usize>, bool)> {
    let bytes = code.as_bytes();
    let ends = |b: u8| matches!(b, b';' | b'|' | b'&' | b'\n' | b'(' | b')');
    let mut out = Vec::new();
    let (mut start, mut piped, mut open) = (0, false, 0usize);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if !ends(b) {
            i += 1;
            continue;
        }
        if !code[start..i].trim().is_empty() {
            out.push((start..i, piped));
        }
        piped = b == b'|' && bytes.get(i + 1) != Some(&b'|') && (i == 0 || bytes[i - 1] != b'|');
        start = i + 1;
        if b == b'(' {
            open += 1;
        } else if b == b')' && open > 0 {
            open -= 1;
            while start < bytes.len() && !ends(bytes[start]) && !bytes[start].is_ascii_whitespace()
            {
                start += 1;
            }
        }
        i = start;
    }
    if !code[start..].trim().is_empty() {
        out.push((start..code.len(), piped));
    }
    out
}

/// One command of a shell line, as [`command_kind`] read it.
struct Command {
    kind: Kind,
    /// Its program, lowercased (`cargo`, `pytest`, `get-content`).
    program: String,
    /// Its first two plain words after the program, lowercased: the
    /// subcommand and the one after it (`test` in `cargo test`, `run test`
    /// in `npm run test`).
    sub: Option<String>,
    next: Option<String>,
    /// A change that builds first: `mvn install` compiles and tests the
    /// code before it installs it.
    builds: bool,
}

/// Verbs whose next word names what runs: `npm run test`, `deno task check`.
const RUNNER_VERBS: &[&str] = &["run", "exec", "task", "run-script"];
/// Build tools whose `install` goal builds the code before it installs it.
const BUILD_TOOLS: &[&str] = &["mvn", "mvnw", "gradle", "gradlew", "make"];

/// What one command does, read by its program (after `sudo`, `env`, `npx`,
/// `uv run`, `python -m` and the like) and its subcommand, never by a word
/// further on. `text` is the command as written, `code` the same bytes with
/// its data blanked.
fn command_kind(text: &str, code: &str) -> Command {
    let mut all: Vec<&str> = code.split_whitespace().collect();
    // PowerShell's `$out=cargo test` is `$out = cargo test`.
    if let Some((name, value)) = all.first().and_then(|w| w.split_once('=')) {
        if name.starts_with('$') && !value.is_empty() {
            all.splice(0..1, [name, "=", value]);
        }
    }
    let mut w = peel(&all);
    // `$out = cargo test` runs the command it keeps; `$n = 3` runs none.
    if w.get(1) == Some(&"=") {
        w = peel(&w[2..]);
        let runs = w.first().is_some_and(|v| {
            v.starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '.' | '/' | '\\' | '~'))
        });
        if !runs {
            w = &[];
        }
    }
    let Some(first) = w.first() else {
        return Command {
            kind: Kind::Setup,
            program: String::new(),
            sub: None,
            next: None,
            builds: false,
        };
    };
    let name = program(first);
    let lower: Vec<String> = w.iter().map(|x| x.to_ascii_lowercase()).collect();
    let mut plain = lower[1..]
        .iter()
        .map(String::as_str)
        .filter(|x| !x.starts_with(['-', '+']));
    let sub = plain.next();
    let next = plain.next();
    let builtin = !first.contains(['/', '\\']);
    let exec_at = (name == "find")
        .then(|| {
            lower
                .iter()
                .position(|x| matches!(x.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
        })
        .flatten();
    let mut builds = false;
    let kind = if SETUP.contains(&name.as_str()) || first.starts_with('-') || ps_expression(w) {
        // `-arg` alone is a continued line.
        Kind::Setup
    } else if (matches!(name.as_str(), "sed" | "perl" | "ruby")
        && w[1..].iter().any(|f| in_place(&name, f)))
        || (name == "find" && lower.iter().any(|x| x == "-delete"))
    {
        Kind::Change
    } else if let Some(at) = exec_at {
        // `find . -exec grep -n x {} +` runs grep on each file it finds.
        let inner = w[at + 1..].join(" ");
        match command_kind(&inner, &inner).kind {
            Kind::Look => Kind::Look,
            Kind::Change => Kind::Change,
            _ => Kind::Exec,
        }
    } else if (builtin && matches!(name.as_str(), "test" | "[" | "[["))
        || lower.iter().any(|x| x == "--version")
    {
        // The shell's `test -f x` only asks.
        Kind::Look
    } else if RUN_WORDS.contains(&name.as_str()) || TEST_PROGRAMS.contains(&name.as_str()) {
        // A check runner's words are its filters: `pytest -k update`.
        Kind::Run
    } else if (builtin && VIEWERS.contains(&name.as_str()))
        || crate::perm::shell_is_readonly(text)
        || LOOK_WORDS.contains(&name.as_str())
        || sub.is_some_and(|s| LOOK_WORDS.contains(&s))
    {
        Kind::Look
    } else if MUTATING_HEADS.contains(&name.as_str())
        || sub.is_some_and(|s| MUTATING_HEADS.contains(&s))
    {
        builds = BUILD_TOOLS.contains(&name.as_str()) && sub == Some("install");
        Kind::Change
    } else if name == "git" {
        // `git fetch` or `git stash` neither checks code nor is a change to check.
        Kind::Setup
    } else if sub.is_some_and(|s| RUN_WORDS.contains(&stem(s)))
        || (sub.is_some_and(|s| RUNNER_VERBS.contains(&s))
            && next.is_some_and(|n| RUN_WORDS.contains(&stem(n))))
    {
        Kind::Run
    } else {
        Kind::Exec
    };
    Command {
        kind,
        program: name,
        sub: sub.map(str::to_string),
        next: next.map(str::to_string),
        builds,
    }
}

/// `$c[0..9]`, `$x.Count` or `$f in …`: a PowerShell expression or loop
/// header, which runs no program. `$PYTHON -m pytest` still runs one.
fn ps_expression(w: &[&str]) -> bool {
    let Some(rest) = w[0].strip_prefix('$') else {
        return false;
    };
    let after =
        rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':');
    after.starts_with(['[', '.']) || w.get(1).is_some_and(|x| x.eq_ignore_ascii_case("in"))
}

/// A script name's first part: `test` in `test:unit` or `test-ci`.
fn stem(word: &str) -> &str {
    word.split([':', '-']).next().unwrap_or(word)
}

/// Programs that run tests. Any program whose subcommand is a test word
/// runs them too (`cargo test`, `go test`, `npm run test`, `cargo nextest`).
const TEST_PROGRAMS: &[&str] = &[
    "pytest", "jest", "vitest", "ctest", "phpunit", "rspec", "unittest",
];
const TEST_SUBS: &[&str] = &["test", "tests", "nextest"];

/// `cmd` runs tests: one of its commands runs a test runner, by its program
/// or its subcommand (`cargo test 2>&1 | tail`, `python -m pytest`, `npm run
/// test:unit`, `yarn jest`). A test word anywhere else runs none: `pip
/// install pytest-mock`, `cat jest.config.js`, `grep test x`, a commit message.
pub(crate) fn runs_tests(cmd: &str) -> bool {
    let code = crate::shell_redirect::code(cmd);
    commands(&code).into_iter().any(|(at, _)| {
        let c = command_kind(&cmd[at.clone()], &code[at]);
        let test = |w: &Option<String>| {
            w.as_deref()
                .map(stem)
                .is_some_and(|w| TEST_SUBS.contains(&w) || TEST_PROGRAMS.contains(&w))
        };
        matches!(c.kind, Kind::Run | Kind::Exec)
            && (TEST_PROGRAMS.contains(&c.program.as_str())
                || test(&c.sub)
                || (c.sub.as_deref().is_some_and(|s| RUNNER_VERBS.contains(&s)) && test(&c.next)))
    })
}

/// `words` past the assignments, wrappers and runners that only start the
/// command after them.
fn peel<'a>(mut w: &'a [&'a str]) -> &'a [&'a str] {
    loop {
        while w.first().is_some_and(|x| assignment(x)) {
            w = &w[1..];
        }
        let Some(first) = w.first() else {
            return w;
        };
        let name = program(first);
        if let Some((_, takes)) = WRAPPERS.iter().find(|(n, _)| *n == name) {
            // `command -v cargo` looks a program up; it runs nothing.
            if name == "command" && w.get(1).is_some_and(|f| *f == "-v" || *f == "-V") {
                return w;
            }
            w = &w[1..];
            while let Some(flag) = w.first().filter(|f| f.starts_with('-')) {
                let n = if takes.contains(flag) { 2 } else { 1 };
                w = &w[n.min(w.len())..];
            }
            if name == "timeout"
                && w.first()
                    .is_some_and(|d| d.starts_with(|c: char| c.is_ascii_digit()))
            {
                w = &w[1..];
            }
            continue;
        }
        let runner = w.get(1).is_some_and(|second| {
            RUNNERS
                .iter()
                .any(|(a, b)| *a == name && second.eq_ignore_ascii_case(b))
        });
        if !runner {
            return w;
        }
        w = &w[2..];
        while w.first().is_some_and(|f| f.starts_with('-')) {
            w = &w[1..];
        }
    }
}

/// `NAME=value` before a command (or PowerShell's `$env:NAME=value`).
fn assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) if name.starts_with('$') => true,
        Some((name, _)) => {
            name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// A command word's program: its file name, lowercased, without `.exe`,
/// `.cmd` or `.bat`.
fn program(word: &str) -> String {
    let base = word
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .to_ascii_lowercase();
    [".exe", ".cmd", ".bat"]
        .iter()
        .find_map(|ext| base.strip_suffix(ext))
        .map_or_else(|| base.clone(), str::to_string)
}

/// `flag` makes `sed`, `perl` or `ruby` edit its files in place: `-i`,
/// `-i.bak`, `-pi`, `--in-place`. A short flag is matched as written, so
/// `-I` (an include folder) and `-Mstrict` (a module) are not it.
fn in_place(name: &str, flag: &str) -> bool {
    if flag.starts_with("--in-place") {
        return true;
    }
    let Some(short) = flag.strip_prefix('-').filter(|f| !f.starts_with('-')) else {
        return false;
    };
    if name != "sed" && short.starts_with(['M', 'm', 'I']) {
        return false;
    }
    short.split('.').next().is_some_and(|f| f.contains('i'))
}

const DESKTOP_WHAT: &str = "the screen";

fn short(cmd: &str) -> String {
    let one = cmd.lines().next().unwrap_or("").trim();
    if one.chars().count() > 60 {
        format!("{}…", one.chars().take(60).collect::<String>())
    } else {
        one.to_string()
    }
}

/// Two spellings of one file: equal once trimmed, with `/` for `\`, no
/// leading `./` and in lowercase, or one absolute path (`/`, `//` or `x:/`)
/// that ends with `/` and the other. A file name alone never matches.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.trim().replace('\\', "/").to_ascii_lowercase();
        p.strip_prefix("./").map(str::to_string).unwrap_or(p)
    };
    let absolute = |p: &str| {
        p.starts_with('/') || (p.len() > 2 && p.as_bytes()[1] == b':' && p.as_bytes()[2] == b'/')
    };
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let tail = |long: &str, short: &str| long.ends_with(&format!("/{short}"));
    a == b
        || match (absolute(&a), absolute(&b)) {
            (true, false) => tail(&a, &b),
            (false, true) => tail(&b, &a),
            _ => false,
        }
}

/// The run's changes and checks so far.
#[derive(Debug, Clone, Default)]
pub struct Fresh {
    /// Every change no check has covered yet, oldest first, and what checks it.
    unchecked: Vec<(String, Needs)>,
    /// The first system change, while no backup came before or after it.
    unbacked: Option<String>,
    backed_up: bool,
    /// This run posted a findings card.
    findings: bool,
    /// The chat this run belongs to, so a backup covers its later turns too.
    conversation: String,
}

/// How long one backup covers a chat: Windows makes at most one restore
/// point a day, so each Do all step asking for its own got skipped anyway.
const BACKUP_COVERS_MS: u64 = 24 * 60 * 60 * 1000;

static BACKED_UP: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::LazyLock::new(Default::default);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A backup or restore point ran in this chat within the last day.
fn backed_up_lately(conversation: &str) -> bool {
    !conversation.is_empty()
        && BACKED_UP
            .lock()
            .ok()
            .and_then(|m| m.get(conversation).copied())
            .is_some_and(|at| now_ms().saturating_sub(at) < BACKUP_COVERS_MS)
}

impl Fresh {
    /// A run in `conversation`: a backup an earlier turn of the chat took
    /// (a Do all batch's first step) still covers this one's changes.
    pub fn for_conversation(conversation: &str) -> Self {
        Self {
            backed_up: backed_up_lately(conversation),
            conversation: conversation.to_string(),
            ..Self::default()
        }
    }

    /// Note one tool call that ran without failing.
    pub fn note(&mut self, name: &str, args: &Value) {
        for step in classify(name, args) {
            match step {
                Step::Change {
                    what,
                    system,
                    needs,
                } => {
                    if system && !self.backed_up && self.unbacked.is_none() {
                        self.unbacked = Some(what.clone());
                    }
                    if !self.unchecked.iter().any(|(w, _)| same_path(w, &what)) {
                        self.unchecked.push((what, needs));
                    }
                }
                Step::Backup => {
                    self.backed_up = true;
                    self.unbacked = None;
                    if !self.conversation.is_empty() {
                        if let Ok(mut m) = BACKED_UP.lock() {
                            m.insert(self.conversation.clone(), now_ms());
                        }
                    }
                }
                step => self
                    .unchecked
                    .retain(|(what, n)| !n.checked_by(what, &step)),
            }
        }
    }

    /// Note a shell command that ran to an exit code other than 0: a test,
    /// build or other program it ran still checked the changes it covers,
    /// since its result says how the check went. A change it may have made
    /// is not assumed.
    pub fn note_failed_run(&mut self, args: &Value) {
        for step in classify("run_terminal_command", args) {
            if matches!(step, Step::Run | Step::Exec) {
                self.unchecked
                    .retain(|(what, n)| !n.checked_by(what, &step));
            }
        }
    }

    /// The changes no check has covered since they ran, oldest first.
    pub fn unchecked(&self) -> &[(String, Needs)] {
        &self.unchecked
    }

    /// Note that `report_findings` posted this run's card.
    pub fn note_findings(&mut self) {
        self.findings = true;
    }
}

/// A careful or critical repair or research run that did tool work ends with
/// a findings card.
fn owes_findings(brief: Option<&Brief>, evidence: usize) -> bool {
    brief.is_some_and(|b| {
        evidence > 0
            && b.care >= Care::Careful
            && matches!(b.intent, Intent::Repair | Intent::Research)
    })
}

/// The Brief's done items a careful or critical task, repair or research run
/// has not covered in its checklist (an item completed or cancelled whose text
/// names it). Empty for other runs and before the run did any tool work.
pub fn uncovered(brief: Option<&Brief>, todos: &[TodoItem], evidence: usize) -> Vec<String> {
    let Some(b) = brief.filter(|b| {
        evidence > 0
            && b.care >= Care::Careful
            && matches!(b.intent, Intent::Task | Intent::Repair | Intent::Research)
    }) else {
        return Vec::new();
    };
    b.done
        .iter()
        .filter(|item| {
            let keys: Vec<String> = words(item)
                .into_iter()
                .map(str::to_ascii_lowercase)
                .filter(|w| !STOP_WORDS.contains(&w.as_str()))
                .collect();
            !todos.iter().any(|t| {
                let text = t.content.to_ascii_lowercase();
                matches!(t.status.as_str(), "completed" | "cancelled")
                    && !keys.is_empty()
                    && keys.iter().all(|k| text.contains(k.as_str()))
            })
        })
        .cloned()
        .collect()
}

/// What the run still owes before it may end, beyond its checklist's open items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gaps {
    /// The Brief's done items not covered yet.
    pub uncovered: Vec<String>,
    /// Every change with no check after it, oldest first, and what checks it.
    pub unchecked: Vec<(String, Needs)>,
    /// A system change in a critical run with no backup.
    pub unbacked: Option<String>,
    /// A repair or research run that has not posted its findings card.
    pub findings: bool,
}

impl Gaps {
    /// A run with no Brief (a bare loop, not a chat turn) owes nothing here.
    pub fn of(brief: Option<&Brief>, todos: &[TodoItem], evidence: usize, fresh: &Fresh) -> Self {
        let critical = brief.is_some_and(|b| b.care == Care::Critical);
        Self {
            uncovered: uncovered(brief, todos, evidence),
            unchecked: if brief.is_some() {
                fresh.unchecked().to_vec()
            } else {
                Vec::new()
            },
            unbacked: fresh.unbacked.clone().filter(|_| critical),
            findings: !fresh.findings && owes_findings(brief, evidence),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.uncovered.is_empty()
            && self.unchecked.is_empty()
            && self.unbacked.is_none()
            && !self.findings
    }

    /// Stable names for the drive's stall count.
    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<String> = self.uncovered.iter().map(|u| format!("done:{u}")).collect();
        k.extend(self.unchecked.iter().map(|(u, _)| format!("unchecked:{u}")));
        k.extend(self.unbacked.iter().map(|u| format!("unbacked:{u}")));
        if self.findings {
            k.push("findings".into());
        }
        k
    }

    /// Plain names for the blocked line.
    pub fn names(&self) -> Vec<String> {
        let mut n: Vec<String> = self.uncovered.iter().map(|u| format!("\"{u}\"")).collect();
        if !self.unchecked.is_empty() {
            n.push(format!("a check after the change to {}", self.listed()));
        }
        n.extend(self.unbacked.iter().map(|u| format!("a backup for {u}")));
        if self.findings {
            n.push("the findings card".into());
        }
        n
    }

    /// The re-plan note, one sentence per gap.
    pub fn note(&self) -> String {
        let mut parts = Vec::new();
        if let Some(what) = &self.unbacked {
            parts.push(format!(
                "Changed {what} with no backup first. Save a copy of the current state now and say how to undo the change."
            ));
        }
        if !self.unchecked.is_empty() {
            let what = self.listed();
            parts.push(if self.unchecked.iter().any(|(_, n)| *n == Needs::Run) {
                format!(
                    "Not verified since your last change to {what}. Run the tests, a build or a linter (or run the changed file) before you finish: a re-read or git status doesn't check code."
                )
            } else {
                format!(
                    "Not verified since your last change to {what}. Check it (run the tests or build, re-read the file, look at the result) before you finish."
                )
            });
        }
        if !self.uncovered.is_empty() {
            let list = self
                .uncovered
                .iter()
                .map(|u| format!("\"{u}\""))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!(
                "This ask is done only when it shows {list}. Put each in your checklist (todo_write) and mark it completed once a tool result shows it, or cancelled with the reason it is blocked."
            ));
        }
        if self.findings {
            parts.push(
                "End with the findings card: call report_findings naming each item you checked and its root cause, with a one-tap fix for what is left."
                    .to_string(),
            );
        }
        parts.join(" ")
    }

    /// The unchecked changes: "a.rs", "a.rs and b.md", "a.rs, b.rs, c.rs and 2 more".
    fn listed(&self) -> String {
        let mut out: Vec<String> = self
            .unchecked
            .iter()
            .take(PROGRESS_ITEMS)
            .map(|(w, _)| w.clone())
            .collect();
        if self.unchecked.len() > PROGRESS_ITEMS {
            out.push(format!("{} more", self.unchecked.len() - PROGRESS_ITEMS));
        }
        join_and(out)
    }
}

/// An item's first line, without a closing period, clipped.
fn clip(text: &str) -> String {
    let one = text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.');
    if one.chars().count() > ITEM_CHARS {
        format!("{}…", one.chars().take(ITEM_CHARS).collect::<String>())
    } else {
        one.to_string()
    }
}

fn closed(t: &TodoItem) -> bool {
    matches!(t.status.as_str(), "completed" | "cancelled")
}

/// An item as it reads mid-sentence: "Check failed services" becomes
/// "check failed services"; "DNS" and "Wi-Fi" stay as they are.
fn mid_sentence(item: &str) -> String {
    let first = item.split_whitespace().next().unwrap_or_default();
    let mut chars = first.chars();
    let plain_word = chars.next().is_some_and(char::is_uppercase)
        && chars.all(|c| c.is_alphabetic() && c.is_lowercase());
    if plain_word {
        let mut c = item.chars();
        c.next()
            .map(|a| a.to_lowercase().chain(c).collect())
            .unwrap_or_default()
    } else {
        item.to_string()
    }
}

/// "a", "a and b", "a, b and c", "a, b, c and 2 others".
fn named(items: &[&TodoItem]) -> String {
    let mut out: Vec<String> = items
        .iter()
        .take(PROGRESS_ITEMS)
        .map(|t| mid_sentence(&clip(&t.content)))
        .collect();
    if items.len() > PROGRESS_ITEMS {
        let rest = items.len() - PROGRESS_ITEMS;
        out.push(format!(
            "{rest} {}",
            if rest == 1 { "other" } else { "others" }
        ));
    }
    join_and(out)
}

/// "a", "a and b", "a, b and c".
fn join_and(out: Vec<String>) -> String {
    match out.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => out.join(""),
    }
}

/// Brain 5 progress notes for a long run, in plain sentences: "Finished
/// services and packages. Now on boot errors." A note comes only at a phase boundary (a checklist item
/// closed since the last note) while an item is still open (the reply covers
/// the end), and only once [`PROGRESS_GAP`] tool calls ran since the last
/// note. A boundary that comes too soon is told with the next one.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Closed items already told, or closed when the run started.
    told: Vec<(String, String)>,
    /// Tool calls at the last note.
    at: usize,
}

impl Progress {
    pub fn new(start: &[TodoItem]) -> Self {
        Self {
            told: start
                .iter()
                .filter(|t| closed(t))
                .map(|t| (t.id.clone(), t.content.clone()))
                .collect(),
            at: 0,
        }
    }

    /// The note to show now, if any. `evidence` counts the run's task tool calls.
    pub fn check(&mut self, todos: &[TodoItem], evidence: usize) -> Option<String> {
        let fresh: Vec<&TodoItem> = todos
            .iter()
            .filter(|t| {
                closed(t)
                    && !self
                        .told
                        .iter()
                        .any(|(id, c)| *id == t.id && *c == t.content)
            })
            .collect();
        if fresh.is_empty() || evidence < self.at + PROGRESS_GAP {
            return None;
        }
        let next = todos
            .iter()
            .find(|t| t.status == "in_progress")
            .or_else(|| todos.iter().find(|t| t.status == "pending"))?;
        let done: Vec<&TodoItem> = fresh
            .iter()
            .copied()
            .filter(|t| t.status == "completed")
            .collect();
        let skipped: Vec<&TodoItem> = fresh
            .iter()
            .copied()
            .filter(|t| t.status == "cancelled")
            .collect();
        let mut parts = Vec::new();
        if !done.is_empty() {
            parts.push(format!("Finished {}.", named(&done)));
        }
        if !skipped.is_empty() {
            parts.push(format!("Skipped {}.", named(&skipped)));
        }
        parts.push(format!("Now on {}.", mid_sentence(&clip(&next.content))));
        self.told
            .extend(fresh.iter().map(|t| (t.id.clone(), t.content.clone())));
        self.at = evidence;
        Some(parts.join(" "))
    }
}

/// The card a repair or research run that owes one ends with when it stops
/// blocked without posting it: each checklist item by what became of it,
/// what the run still owed, and a one-tap fix to finish the rest. `None` when
/// there is nothing to name.
pub fn fallback_findings(todos: &[TodoItem], gaps: &Gaps) -> Option<Findings> {
    let mut items: Vec<Finding> = Vec::new();
    let mut open: Vec<String> = Vec::new();
    for t in todos {
        let text = clip(&t.content);
        let (severity, said) = match t.status.as_str() {
            "completed" => (Severity::Info, format!("Done: {text}")),
            "cancelled" => (Severity::Low, format!("Skipped: {text}")),
            _ => {
                open.push(text.clone());
                (Severity::Medium, format!("Not finished: {text}"))
            }
        };
        items.push(Finding {
            severity,
            text: said,
        });
    }
    for u in &gaps.uncovered {
        open.push(clip(u));
        items.push(Finding {
            severity: Severity::Medium,
            text: format!("Not shown yet: {}", clip(u)),
        });
    }
    if !gaps.unchecked.is_empty() {
        items.push(Finding {
            severity: Severity::Medium,
            text: format!("Not checked since the change to {}", gaps.listed()),
        });
    }
    if let Some(u) = &gaps.unbacked {
        items.push(Finding {
            severity: Severity::High,
            text: format!("No backup before {u}"),
        });
    }
    if items.is_empty() {
        return None;
    }
    items.sort_by_key(|f| f.severity);
    items.truncate(MAX_FINDINGS);
    // Skip open todos that are impossible one-taps (grant workspace / activate
    // skill / …). #720 refused those only on report_findings → from_json; the
    // stalled-run fallback built the pill directly and re-offered the doom loop.
    let doable: Vec<String> = open
        .into_iter()
        .filter(|t| !is_impossible_one_tap("", t))
        .collect();
    let fixes = if doable.is_empty() {
        Vec::new()
    } else {
        let goal = format!("Finish what the last run left open: {}.", doable.join("; "));
        if is_impossible_one_tap("Finish the rest", &goal) {
            Vec::new()
        } else {
            vec![Fix {
                label: "Finish the rest".into(),
                goal,
            }]
        }
    };
    Some(Findings { items, fixes })
}

/// The turn's Brief, by chat session, for the run loop's checkpoints.
/// Each per-session map drops its least recently used chat when full.
static BRIEFS: Mutex<LruMap<Brief>> = Mutex::new(LruMap::new(BRIEFS_CAP));
const BRIEFS_CAP: usize = 256;

pub fn set_turn_brief(session: &str, brief: Brief) {
    if session.is_empty() {
        return;
    }
    let mut m = BRIEFS.lock().unwrap_or_else(|e| e.into_inner());
    m.insert(session, brief);
}

pub fn turn_brief(session: &str) -> Option<Brief> {
    BRIEFS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .cloned()
}

/// The turn's talk style, by chat session (Brain 5). `None` clears it.
static STYLES: Mutex<LruMap<TalkStyle>> = Mutex::new(LruMap::new(BRIEFS_CAP));

pub fn set_turn_style(session: &str, style: Option<TalkStyle>) {
    if session.is_empty() {
        return;
    }
    let mut m = STYLES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(style) = style else {
        m.remove(session);
        return;
    };
    m.insert(session, style);
}

pub fn turn_style(session: &str) -> Option<TalkStyle> {
    STYLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .copied()
}

/// The turn's playbook paragraph, by chat session (Brain 3). `None` clears it.
static PLAYBOOKS: Mutex<LruMap<String>> = Mutex::new(LruMap::new(BRIEFS_CAP));

pub fn set_turn_playbook(session: &str, block: Option<String>) {
    if session.is_empty() {
        return;
    }
    let mut m = PLAYBOOKS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(block) = block else {
        m.remove(session);
        return;
    };
    m.insert(session, block);
}

pub fn turn_playbook(session: &str) -> Option<String> {
    PLAYBOOKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .cloned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn shell(cmd: &str) -> Value {
        json!({ "command": cmd })
    }

    fn todo(content: &str, status: &str) -> TodoItem {
        TodoItem {
            id: content.into(),
            content: content.into(),
            status: status.into(),
        }
    }

    /// The changes still waiting on a check, oldest first.
    fn names(f: &Fresh) -> Vec<&str> {
        f.unchecked().iter().map(|(w, _)| w.as_str()).collect()
    }

    #[test]
    fn a_change_waits_for_a_check_that_fits_it() {
        let mut f = Fresh::default();
        f.note("search_replace", &json!({ "file_path": "src/main.rs" }));
        assert_eq!(names(&f), ["src/main.rs"]);
        // A look at something else doesn't check a file edit.
        f.note("run_terminal_command", &shell("ls -la"));
        f.note("read_file", &json!({ "target_file": "src/lib.rs" }));
        assert_eq!(names(&f), ["src/main.rs"]);
        // Re-reading changed code doesn't check it either; a test run does.
        f.note("read_file", &json!({ "target_file": "./src/main.rs" }));
        assert_eq!(names(&f), ["src/main.rs"]);
        f.note("write", &json!({ "path": "a.txt" }));
        f.note(
            "run_terminal_command",
            &shell("cargo test -p grokhub-agent"),
        );
        assert_eq!(names(&f), Vec::<&str>::new());
        // A shell change is checked by any read-only look; a desktop action by a screenshot.
        f.note("run_terminal_command", &shell("systemctl restart nginx"));
        assert_eq!(names(&f), ["systemctl restart nginx"]);
        f.note("run_terminal_command", &shell("ls /etc/nginx"));
        assert_eq!(names(&f), Vec::<&str>::new());
        f.note("click", &json!({ "x": 1, "y": 2 }));
        f.note("run_terminal_command", &shell("ls"));
        assert_eq!(names(&f), ["the screen"]);
        f.note("screenshot", &json!({}));
        assert_eq!(names(&f), Vec::<&str>::new());
        // A fetch or a query changes nothing; an install or a redirect does.
        f.note(
            "run_terminal_command",
            &shell("curl -s https://wttr.in/?format=3"),
        );
        f.note("run_terminal_command", &shell("python3 -c 'print(1)'"));
        assert_eq!(names(&f), Vec::<&str>::new());
        f.note("run_terminal_command", &shell("pip install requests"));
        assert_eq!(names(&f), ["pip install requests"]);
        f.note("run_terminal_command", &shell("echo hi > out.txt"));
        assert_eq!(names(&f), ["pip install requests", "echo hi > out.txt"]);
        // A check word after the change in the same command checks it.
        f.note(
            "run_terminal_command",
            &shell("sed -i 's/a/b/' x.py && pytest"),
        );
        assert_eq!(names(&f), Vec::<&str>::new());
        f.note("run_terminal_command", &shell("pytest && rm -rf build/"));
        assert_eq!(names(&f), ["pytest && rm -rf build/"]);
        // PowerShell names in any case.
        f.note("run_terminal_command", &shell("Get-Service spooler"));
        assert_eq!(names(&f), Vec::<&str>::new());
        f.note(
            "run_terminal_command",
            &shell("remove-item C:\\temp\\x.txt"),
        );
        assert_eq!(names(&f), ["remove-item C:\\temp\\x.txt"]);
        // A workspace file named like a backup is still a change.
        f.note("write", &json!({ "path": "notes/backup-plan.md" }));
        assert_eq!(
            names(&f),
            ["remove-item C:\\temp\\x.txt", "notes/backup-plan.md"]
        );
    }

    #[test]
    fn a_code_change_needs_a_run_not_a_reread_or_a_status_look() {
        let mut f = Fresh::default();
        f.note("search_replace", &json!({ "file_path": "src/handler.rs" }));
        f.note("read_file", &json!({ "target_file": "src/handler.rs" }));
        for look in [
            "git status",
            "git diff src/handler.rs",
            "cat src/handler.rs",
        ] {
            f.note("run_terminal_command", &shell(look));
        }
        assert_eq!(names(&f), ["src/handler.rs"]);
        assert_eq!(f.unchecked()[0].1, Needs::Run);
        // Each change waits on its own, oldest first.
        f.note("write", &json!({ "path": "docs/notes.md" }));
        f.note("search_replace", &json!({ "file_path": "tools/gen.py" }));
        assert_eq!(
            names(&f),
            ["src/handler.rs", "docs/notes.md", "tools/gen.py"]
        );
        // A file name alone is not the file; the same file by its full path is.
        f.note("read_file", &json!({ "target_file": "other/notes.md" }));
        assert_eq!(
            names(&f),
            ["src/handler.rs", "docs/notes.md", "tools/gen.py"]
        );
        f.note(
            "read_file",
            &json!({ "target_file": "/home/user/ws/docs/notes.md" }),
        );
        assert_eq!(names(&f), ["src/handler.rs", "tools/gen.py"]);
        // Any command that runs checks code, but not a doc: that takes a re-read or a check run.
        f.note("write", &json!({ "path": "docs/notes.md" }));
        f.note("run_terminal_command", &shell("python3 tools/gen.py"));
        assert_eq!(names(&f), ["docs/notes.md"]);
        f.note("run_terminal_command", &shell("cargo build -p app"));
        assert_eq!(names(&f), Vec::<&str>::new());
        // A redirect that only moves or drops output is no change: these are runs.
        for run in [
            "cargo test 2>&1 | tail -20",
            "npm test > /dev/null 2>&1",
            "npm test 2>/dev/null",
            "cargo build | Out-Null",
            "cargo test *>$null",
            "Invoke-Pester >$null",
        ] {
            f.note("search_replace", &json!({ "file_path": "src/handler.rs" }));
            f.note("run_terminal_command", &shell(run));
            assert_eq!(names(&f), Vec::<&str>::new(), "{run}");
        }
        // A redirect to a file is a change: the run before it still checks the code.
        f.note("search_replace", &json!({ "file_path": "src/handler.rs" }));
        f.note("run_terminal_command", &shell("cargo test > test.log"));
        assert_eq!(names(&f), ["cargo test > test.log"]);
        f.note("run_terminal_command", &shell("cat test.log"));
        assert_eq!(names(&f), Vec::<&str>::new());
        // Windows spellings of the same file.
        f.note("write", &json!({ "path": "C:\\ws\\docs\\Notes.md" }));
        f.note("read_file", &json!({ "target_file": "docs/notes.md" }));
        assert_eq!(names(&f), Vec::<&str>::new());
    }

    /// Each command after a fresh edit to `src/handler.rs`, and what is left
    /// unchecked after it.
    fn left_after_code_edit(cases: &[(&str, &[&str])]) {
        for (cmd, left) in cases {
            let mut f = Fresh::default();
            f.note("search_replace", &json!({ "file_path": "src/handler.rs" }));
            f.note("run_terminal_command", &shell(cmd));
            assert_eq!(names(&f), *left, "{cmd}");
        }
    }

    #[test]
    fn a_shell_line_is_read_by_the_program_of_each_command() {
        let code: &[&str] = &["src/handler.rs"];
        left_after_code_edit(&[
            // A viewer only looks, however it is spelled or wherever it runs.
            ("sed -n '1,60p' src/handler.rs", code),
            ("Get-Content src\\handler.rs", code),
            ("cd crates/app && cat src/lib.rs", code),
            ("cd src && cat handler.rs", code),
            ("find . -name '*.rs'", code),
            ("echo hi", code),
            ("awk 'NR<10' src/handler.rs", code),
            ("nl src/handler.rs", code),
            ("cat src/handler.rs", code),
            ("command -v cargo", code),
            ("git log --oneline -5", code),
            // A check word in quotes, a here-document or an argument runs nothing.
            (
                "git add -A && git commit -m \"Fix the failing test\"",
                &[
                    "src/handler.rs",
                    "git add -A && git commit -m \"Fix the failing test\"",
                ],
            ),
            (
                "git commit -m \"fix test\"",
                &["src/handler.rs", "git commit -m \"fix test\""],
            ),
            ("grep -n \"test\" src/handler.rs", code),
            ("grep -n test src/handler.rs", code),
            ("ls tests", code),
            (
                "cat > notes.md <<'EOF2'\nRemove the old build step\nEOF2",
                &["src/handler.rs", "cat > notes.md <<'EOF2'"],
            ),
            // A program that runs gets the credit, even when it writes a log.
            ("make", &[]),
            ("make > build.log 2>&1", &["make > build.log 2>&1"]),
            (
                "python3 tools/gen.py > out.json",
                &["python3 tools/gen.py > out.json"],
            ),
            ("make 2>&1 | tee build.log", &["make 2>&1 | tee build.log"]),
            (
                "npx vitest run 2>&1 | tee vitest.log",
                &["npx vitest run 2>&1 | tee vitest.log"],
            ),
            ("npm install && npm run typecheck", &[]),
            (
                "npm install && node src/handler.js",
                &["npm install && node src/handler.js"],
            ),
            ("RUST_LOG=debug sudo -u app timeout 60 cargo test", &[]),
            ("uv run --frozen pytest -q", &[]),
            ("python -m pytest tests/", &[]),
            ("cargo test \\\n  --workspace", &[]),
            // A change is a program or subcommand, never an argument or a grep flag.
            ("cargo test 2>&1 | grep -i fail", &[]),
            ("cargo test 2>&1 | grep -i error", &[]),
            ("cargo test 2>&1 | rg -i error", &[]),
            ("cargo test update", &[]),
            ("pytest -k \"add or remove\"", &[]),
            (
                "sed -i.bak 's/a/b/' src/handler.rs",
                &["src/handler.rs", "sed -i.bak 's/a/b/' src/handler.rs"],
            ),
            (
                "perl -pi -e 's/a/b/' x.txt",
                &["src/handler.rs", "perl -pi -e 's/a/b/' x.txt"],
            ),
            ("perl -Mstrict -e 'print 1'", &[]),
            (
                "find . -name '*.orig' -delete",
                &["src/handler.rs", "find . -name '*.orig' -delete"],
            ),
            // A check runner's own words are filters, never a change or a look.
            ("pytest -k update", &[]),
            ("npx jest -t add", &[]),
            ("pytest -k status", &[]),
            ("python -m pytest -k remove", &[]),
            // A PowerShell variable keeps what its command printed.
            (
                "$out = cargo test 2>&1 | Out-String; Write-Output $out",
                &[],
            ),
            ("$out=cargo test", &[]),
            ("$n = 3", code),
            ("$env:RUST_LOG=\"debug\"", code),
            // An index, a property or a path after `)` is data, and so is a loop header.
            ("(Get-Content src\\handler.rs)[0..40]", code),
            ("(Get-Content src\\handler.rs).Count", code),
            ("cat $(pwd)/src/handler.rs", code),
            ("$c = Get-Content src\\handler.rs; $c[10..30]", code),
            (
                "foreach ($f in Get-ChildItem *.rs) { Get-Content $f }",
                code,
            ),
            (
                "Get-ChildItem *.rs | ForEach-Object { Get-Content $_ }",
                code,
            ),
            ("find . -name '*.rs' -exec grep -n 'fn main' {} +", code),
            // A viewer a change pipes into shows the change's lines, not its result.
            (
                "pip install requests 2>&1 | tail -3",
                &["src/handler.rs", "pip install requests 2>&1 | tail -3"],
            ),
            (
                "systemctl restart nginx && systemctl status nginx | head",
                code,
            ),
            // The shell's `test` and `[` only ask, and so does a package lookup.
            ("test -f package.json && echo yes", code),
            ("[ -f src/handler.rs ] && echo ok", code),
            ("pip show pytest", code),
            // `mvn install` builds the code, then installs it.
            ("mvn install", &["mvn install"]),
            (
                "Add-Content src\\handler.rs x",
                &["src/handler.rs", "Add-Content src\\handler.rs x"],
            ),
        ]);
        // A .py edit is the same: the generator ran, and only its output file is left.
        let mut f = Fresh::default();
        f.note("write", &json!({ "path": "tools/gen.py" }));
        f.note(
            "run_terminal_command",
            &shell("python3 tools/gen.py > out.json"),
        );
        assert_eq!(names(&f), ["python3 tools/gen.py > out.json"]);
    }

    #[test]
    fn the_note_names_every_unchecked_change_and_says_code_needs_a_run() {
        let task = Brief {
            intent: Intent::Task,
            care: Care::Casual,
            ..Brief::default()
        };
        let mut f = Fresh::default();
        f.note("search_replace", &json!({ "file_path": "src/handler.rs" }));
        f.note("read_file", &json!({ "target_file": "src/handler.rs" }));
        let gaps = Gaps::of(Some(&task), &[], 2, &f);
        assert_eq!(gaps.keys(), ["unchecked:src/handler.rs"]);
        assert_eq!(gaps.names(), ["a check after the change to src/handler.rs"]);
        assert_eq!(
            gaps.note(),
            "Not verified since your last change to src/handler.rs. Run the tests, a build or a linter (or run the changed file) before you finish: a re-read or git status doesn't check code."
        );
        let mut many = Fresh::default();
        for path in ["a.rs", "b.rs", "c.rs", "d.rs"] {
            many.note("write", &json!({ "path": path }));
        }
        let gaps = Gaps::of(Some(&task), &[], 4, &many);
        assert_eq!(
            gaps.names(),
            ["a check after the change to a.rs, b.rs, c.rs and 1 more"]
        );
        assert_eq!(gaps.keys().len(), 4);
        // A doc and a shell change keep the plain note, naming both.
        let mut plain = Fresh::default();
        plain.note("write", &json!({ "path": "docs/notes.md" }));
        plain.note("run_terminal_command", &shell("systemctl restart nginx"));
        assert_eq!(
            Gaps::of(Some(&task), &[], 2, &plain).note(),
            "Not verified since your last change to docs/notes.md and systemctl restart nginx. Check it (run the tests or build, re-read the file, look at the result) before you finish."
        );
    }

    #[test]
    fn a_critical_system_change_needs_a_backup() {
        // A critical task: a repair would also owe its findings card (tested below).
        let critical = Brief {
            intent: Intent::Task,
            care: Care::Critical,
            ..Brief::default()
        };
        let mut f = Fresh::default();
        f.note(
            "run_terminal_command",
            &shell("sudo sed -i 's/a/b/' /etc/fstab"),
        );
        f.note("run_terminal_command", &shell("systemctl status fstab"));
        let gaps = Gaps::of(Some(&critical), &[], 2, &f);
        assert_eq!(
            gaps.unbacked.as_deref(),
            Some("sudo sed -i 's/a/b/' /etc/fstab")
        );
        assert_eq!(gaps.unchecked, []);
        assert_eq!(
            gaps.note(),
            "Changed sudo sed -i 's/a/b/' /etc/fstab with no backup first. Save a copy of the current state now and say how to undo the change."
        );
        // A careful run doesn't need one; a backup (before or after) clears it.
        let careful = Brief {
            care: Care::Careful,
            ..critical.clone()
        };
        assert_eq!(Gaps::of(Some(&careful), &[], 2, &f).unbacked, None);
        f.note(
            "run_terminal_command",
            &shell("sudo cp /etc/fstab /etc/fstab.bak"),
        );
        assert_eq!(Gaps::of(Some(&critical), &[], 3, &f), Gaps::default());
        // A system change hidden behind a check word still needs its backup; a status look is no change.
        let mut h = Fresh::default();
        h.note("run_terminal_command", &shell("systemctl status nginx"));
        assert_eq!(Gaps::of(Some(&critical), &[], 1, &h), Gaps::default());
        h.note(
            "run_terminal_command",
            &shell("sudo systemctl restart nginx && systemctl is-active nginx"),
        );
        assert_eq!(
            Gaps::of(Some(&critical), &[], 2, &h).unbacked.as_deref(),
            Some("sudo systemctl restart nginx && systemctl is-active nginx")
        );
        assert_eq!(h.unchecked(), []);
        // Writing a .bak in the workspace is not a system backup.
        h.note("write", &json!({ "path": "fstab.bak" }));
        assert!(Gaps::of(Some(&critical), &[], 3, &h).unbacked.is_some());
        // A backup an earlier turn of the same chat took covers this run too.
        let conv = format!("backup-covers-{}", std::process::id());
        let mut first = Fresh::for_conversation(&conv);
        first.note(
            "run_terminal_command",
            &serde_json::json!({"command": "Checkpoint-Computer -Description before"}),
        );
        let mut next = Fresh::for_conversation(&conv);
        next.note(
            "run_terminal_command",
            &serde_json::json!({"command": "sudo sed -i x /etc/fstab"}),
        );
        assert_eq!(Gaps::of(Some(&critical), &[], 2, &next).unbacked, None);
        let other = Fresh::for_conversation("another-chat");
        assert!(!other.backed_up);
        // Backed up first: the change never counts as unbacked.
        let mut g = Fresh::default();
        g.note("run_terminal_command", &shell("timeshift --create"));
        g.note("write", &json!({ "path": "/etc/hosts" }));
        assert_eq!(Gaps::of(Some(&critical), &[], 2, &g).unbacked, None);
        // A bare loop with no Brief owes nothing.
        assert_eq!(Gaps::of(None, &[], 2, &g), Gaps::default());
    }

    #[test]
    fn the_done_list_counts_only_when_the_checklist_names_each_item() {
        let b = Brief {
            intent: Intent::Repair,
            care: Care::Critical,
            done: vec!["services".into(), "boot errors".into(), "ports".into()],
            ..Brief::default()
        };
        let todos = [
            todo("Check failed services", "completed"),
            todo("Boot errors in journalctl", "cancelled"),
            todo("open ports", "pending"),
        ];
        assert_eq!(uncovered(Some(&b), &todos, 3), vec!["ports".to_string()]);
        // No tool work yet, a casual run, a question or background job: nothing owed.
        assert!(uncovered(Some(&b), &[], 0).is_empty());
        assert!(uncovered(
            Some(&Brief {
                care: Care::Casual,
                ..b.clone()
            }),
            &[],
            3
        )
        .is_empty());
        assert!(uncovered(
            Some(&Brief {
                intent: Intent::Question,
                ..b.clone()
            }),
            &[],
            3
        )
        .is_empty());
        assert!(uncovered(
            Some(&Brief {
                intent: Intent::Background,
                ..b
            }),
            &[],
            3
        )
        .is_empty());
    }

    #[test]
    fn the_turn_brief_is_kept_per_session() {
        let b = Brief {
            intent: Intent::Task,
            ..Brief::default()
        };
        set_turn_brief("cp-test-a", b.clone());
        set_turn_brief("", Brief::default());
        assert_eq!(turn_brief("cp-test-a"), Some(b));
        assert_eq!(turn_brief(""), None);
        set_turn_style("cp-test-a", Some(TalkStyle::Findings));
        set_turn_style("", Some(TalkStyle::Short));
        assert_eq!(turn_style("cp-test-a"), Some(TalkStyle::Findings));
        assert_eq!(turn_style(""), None);
        set_turn_style("cp-test-a", None);
        assert_eq!(turn_style("cp-test-a"), None);
        set_turn_playbook("cp-test-a", Some("Playbook for this turn: Files.".into()));
        set_turn_playbook("", Some("Playbook for this turn: Research.".into()));
        assert_eq!(
            turn_playbook("cp-test-a").as_deref(),
            Some("Playbook for this turn: Files.")
        );
        assert_eq!(turn_playbook(""), None);
        set_turn_playbook("cp-test-a", None);
        assert_eq!(turn_playbook("cp-test-a"), None);
    }

    #[test]
    fn progress_notes_come_only_at_phase_boundaries_and_are_rate_limited() {
        let start = [
            todo("Old cleanup", "completed"),
            todo("Check failed services", "pending"),
            todo("Verify packages", "pending"),
            todo("Boot errors.", "pending"),
        ];
        let mut p = Progress::new(&start);
        // Nothing closed yet: no note.
        assert_eq!(p.check(&start, 1), None);
        let mut now = start.to_vec();
        now[1].status = "completed".into();
        // A boundary after 2 tool calls waits for the gap, then is told.
        assert_eq!(p.check(&now, 2), None);
        assert_eq!(
            p.check(&now, 3).as_deref(),
            Some("Finished check failed services. Now on verify packages.")
        );
        // Told once: more work with no new boundary says nothing.
        assert_eq!(p.check(&now, 4), None);
        now[2].status = "completed".into();
        now.push(todo("Firewall", "cancelled"));
        now[3].status = "in_progress".into();
        assert_eq!(p.check(&now, 5), None, "only 2 calls since the last note");
        assert_eq!(
            p.check(&now, 6).as_deref(),
            Some("Finished verify packages. Skipped firewall. Now on boot errors.")
        );
        // The last item closing gets no note: the reply covers the end.
        now[3].status = "completed".into();
        assert_eq!(p.check(&now, 30), None);
        // Many items at once are named up to three.
        let many: Vec<TodoItem> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|n| todo(n, "completed"))
            .chain([todo("f", "pending")])
            .collect();
        assert_eq!(
            Progress::default().check(&many, 3).as_deref(),
            Some("Finished a, b, c and 2 others. Now on f.")
        );
        // One more than three reads "1 other", never "1 others".
        let four: Vec<TodoItem> = ["a", "b", "c", "d"]
            .iter()
            .map(|n| todo(n, "completed"))
            .chain([todo("f", "pending")])
            .collect();
        assert_eq!(
            Progress::default().check(&four, 3).as_deref(),
            Some("Finished a, b, c and 1 other. Now on f.")
        );
        let pair = [
            todo("DNS lookups", "completed"),
            todo("Wi-Fi signal", "completed"),
            todo("Restart the router", "pending"),
        ];
        assert_eq!(
            Progress::default().check(&pair, 3).as_deref(),
            Some("Finished DNS lookups and Wi-Fi signal. Now on restart the router.")
        );
    }

    #[test]
    fn a_careful_repair_or_research_run_owes_its_findings_card() {
        let repair = Brief {
            intent: Intent::Repair,
            care: Care::Careful,
            ..Brief::default()
        };
        let mut f = Fresh::default();
        let gaps = Gaps::of(Some(&repair), &[], 1, &f);
        assert!(gaps.findings);
        assert_eq!(gaps.keys(), vec!["findings".to_string()]);
        assert_eq!(gaps.names(), vec!["the findings card".to_string()]);
        assert_eq!(
            gaps.note(),
            "End with the findings card: call report_findings naming each item you checked and its root cause, with a one-tap fix for what is left."
        );
        assert!(
            Gaps::of(
                Some(&Brief {
                    intent: Intent::Research,
                    ..repair.clone()
                }),
                &[],
                1,
                &f
            )
            .findings
        );
        // No tool work, a casual repair, a task or no Brief: nothing owed.
        assert!(!Gaps::of(Some(&repair), &[], 0, &f).findings);
        assert!(
            !Gaps::of(
                Some(&Brief {
                    care: Care::Casual,
                    ..repair.clone()
                }),
                &[],
                1,
                &f
            )
            .findings
        );
        assert!(
            !Gaps::of(
                Some(&Brief {
                    intent: Intent::Task,
                    ..repair.clone()
                }),
                &[],
                1,
                &f
            )
            .findings
        );
        assert!(!Gaps::of(None, &[], 1, &f).findings);
        f.note_findings();
        assert_eq!(Gaps::of(Some(&repair), &[], 1, &f), Gaps::default());
    }

    #[test]
    fn a_blocked_repair_falls_back_to_a_card_naming_each_item() {
        let todos = [
            todo("Check failed services.", "completed"),
            todo("Firewall", "cancelled"),
            todo("Boot errors\nfrom journalctl", "pending"),
        ];
        let gaps = Gaps {
            uncovered: vec!["ports".into()],
            unchecked: vec![("systemctl restart nginx".into(), Needs::Look)],
            unbacked: Some("sudo sed -i x /etc/fstab".into()),
            findings: true,
        };
        let card = fallback_findings(&todos, &gaps).unwrap();
        let body = "Findings from this run\n\n\
                    - High: No backup before sudo sed -i x /etc/fstab\n\
                    - Medium: Not finished: Boot errors\n\
                    - Medium: Not shown yet: ports\n\
                    - Medium: Not checked since the change to systemctl restart nginx\n\
                    - Low: Skipped: Firewall\n\
                    - Info: Done: Check failed services\n\n\
                    One-tap fixes\n\n\
                    - Finish the rest: Finish what the last run left open: Boot errors; ports.";
        assert_eq!(card.to_body(), body);
        assert_eq!(Findings::parse(body), Some(card));
        // All done: the card names the items and offers no fix; nothing at all: no card.
        let done = fallback_findings(&todos[..1], &Gaps::default()).unwrap();
        assert_eq!(
            done.to_body(),
            "Findings from this run\n\n- Info: Done: Check failed services"
        );
        assert_eq!(
            fallback_findings(
                &[],
                &Gaps {
                    findings: true,
                    ..Gaps::default()
                }
            ),
            None
        );
    }

    #[test]
    fn fallback_findings_omits_impossible_one_tap_finish_pills() {
        let todos = [
            todo("Activate skill steamlibreview so it loads", "pending"),
            todo("Grant workspace access to ~/.steam", "pending"),
            todo("Restart nginx and confirm it is listening", "pending"),
        ];
        let card = fallback_findings(&todos, &Gaps::default()).unwrap();
        assert_eq!(card.fixes.len(), 1, "{:?}", card.fixes);
        assert_eq!(card.fixes[0].label, "Finish the rest");
        assert_eq!(
            card.fixes[0].goal,
            "Finish what the last run left open: Restart nginx and confirm it is listening."
        );
        assert!(
            !card.fixes[0]
                .goal
                .to_ascii_lowercase()
                .contains("activate skill"),
            "{}",
            card.fixes[0].goal
        );
        assert!(
            !card.fixes[0]
                .goal
                .to_ascii_lowercase()
                .contains("grant workspace"),
            "{}",
            card.fixes[0].goal
        );
        // All open work is impossible: keep the findings, offer no pill.
        let doom = fallback_findings(
            &[todo("Enable skill gamingprofile", "pending")],
            &Gaps::default(),
        )
        .unwrap();
        assert!(doom.fixes.is_empty(), "{:?}", doom.fixes);
        assert_eq!(doom.items.len(), 1);
    }
}
