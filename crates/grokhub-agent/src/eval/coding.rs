//! Coding eval v1: 30 fixed tasks reduced from merged GrokHub and tech-bench
//! pull requests. Each task is one Rust file plus a test file. Pass or fail
//! is `rustc --test` on that file, after the test file is restored so a run
//! cannot delete or loosen the tests.
//!
//! The set is selected with `live_eval --suite coding`. One run is one model
//! (`--provider local` or `--provider grok`). The report and `<out>.results.json`
//! record that model's pass rate, escalations, tokens and wall time.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One fixed coding task. `bug` is the seeded file. The tests decide pass or fail.
pub struct CodingFixture {
    pub id: &'static str,
    /// Merged pull request this task is reduced from.
    pub source: &'static str,
    pub turns: &'static [&'static str],
    pub bug: &'static str,
    pub tests: &'static str,
}

const ASK: &[&str] = &[
    "Make the tests in tests/check.rs pass. Change only src/bug.rs. Leave tests/check.rs as it is.",
];

const ORACLE_SECS: u64 = 90;

/// The 30 tasks, in a stable order.
pub fn fixtures() -> &'static [CodingFixture] {
    &FIXTURES
}

pub fn fixture(id: &str) -> Option<&'static CodingFixture> {
    FIXTURES.iter().find(|fix| fix.id == id)
}

/// Write the buggy source and the tests into `ws`.
pub fn seed_workspace(ws: &Path, fix: &CodingFixture) -> std::io::Result<()> {
    write_rel(ws, "src/bug.rs", fix.bug)?;
    write_rel(ws, "tests/check.rs", fix.tests)
}

fn write_rel(ws: &Path, rel: &str, body: &str) -> std::io::Result<()> {
    let path = ws.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)
}

/// Put the original tests back, then compile and run them.
/// A wiped or edited test file is overwritten first, so it cannot decide the score.
pub fn grade(ws: &Path, fix: &CodingFixture) -> Result<(), String> {
    write_rel(ws, "tests/check.rs", fix.tests).map_err(|err| err.to_string())?;
    run_oracle(ws)
}

fn run_oracle(ws: &Path) -> Result<(), String> {
    let bin = ws.join("eval-test-bin");
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(ws.join("eval-test-bin.exe"));
    let compile = logged(
        Command::new("rustc")
            .current_dir(ws)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .args(["--edition", "2021", "--test", "--color", "never"])
            .arg(ws.join("tests/check.rs"))
            .arg("-o")
            .arg(&bin)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    if !compile.status.success() {
        return Err(tail("tests did not compile", &compile.stderr));
    }
    let program = if bin.is_file() {
        bin
    } else {
        let exe = ws.join("eval-test-bin.exe");
        if exe.is_file() {
            exe
        } else {
            return Err("rustc produced no test binary".into());
        }
    };
    let ran = logged(
        Command::new(&program)
            .current_dir(ws)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    if ran.status.success() {
        Ok(())
    } else {
        let mut both = ran.stdout;
        both.extend_from_slice(&ran.stderr);
        Err(tail("tests failed", &both))
    }
}

struct Logged {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
        }
        buf
    })
}

fn logged(cmd: &mut Command) -> Result<Logged, String> {
    let mut child = cmd.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "rustc was not on PATH".to_string()
        } else {
            err.to_string()
        }
    })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > Duration::from_secs(ORACLE_SECS) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err("the test run timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return Err(err.to_string()),
        }
    };
    Ok(Logged {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn tail(label: &str, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.replace('\n', " ");
    let text = text.trim();
    let end = text.chars().rev().take(280).collect::<String>();
    let end: String = end.chars().rev().collect();
    format!("{label}: {end}")
}

/// Calls whose model differs from the try's first model. A try that stays on
/// one model counts as zero.
pub fn count_escalations(tries: &[Vec<&str>]) -> u32 {
    let mut n = 0u32;
    for models in tries {
        let Some(start) = models.iter().copied().find(|model| !model.is_empty()) else {
            continue;
        };
        n += models
            .iter()
            .filter(|model| !model.is_empty() && **model != start)
            .count() as u32;
    }
    n
}

/// Headline numbers for one model on this suite.
pub struct CodingTotals<'a> {
    pub model: &'a str,
    pub provider: &'a str,
    pub task_set: usize,
    pub tries: usize,
    pub passed: usize,
    pub escalations: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub wall_ms: u128,
    pub p50_wall_ms: u128,
    /// Tries that ran no tool, so never read or edited the code.
    pub no_tool_tries: usize,
}

pub fn pass_rate(passed: usize, tries: usize) -> String {
    if tries == 0 {
        "n/a".to_string()
    } else {
        format!(
            "{:.0}% ({passed}/{tries})",
            passed as f64 * 100.0 / tries as f64
        )
    }
}

pub fn render_section(totals: &CodingTotals<'_>) -> String {
    let mut out = format!(
        "## Coding eval v1\n\n\
         One model per run. Pass or fail is the task's own tests.\n\n\
         | model | provider | pass rate | escalations | tokens | input tokens | output tokens | wall ms | p50 wall ms | tries with no tool call |\n\
         |---|---|---|---|---|---|---|---|---|---|\n\
         | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n\n",
        totals.model,
        totals.provider,
        pass_rate(totals.passed, totals.tries),
        totals.escalations,
        totals.input_tokens.saturating_add(totals.output_tokens),
        totals.input_tokens,
        totals.output_tokens,
        totals.wall_ms,
        totals.p50_wall_ms,
        totals.no_tool_tries,
    );
    if totals.tries > 0 && totals.no_tool_tries == totals.tries {
        out.push_str(
            "No try made a tool call, so this measures the model server, not the model. \
             Check that the server returns structured tool calls.\n\n",
        );
    }
    out
}

pub fn results_json(totals: &CodingTotals<'_>) -> String {
    format!(
        "{{\n  \
         \"suite\": \"coding-v1\",\n  \
         \"model\": \"{}\",\n  \
         \"provider\": \"{}\",\n  \
         \"task_set\": {},\n  \
         \"tries\": {},\n  \
         \"passed\": {},\n  \
         \"pass_rate\": \"{}\",\n  \
         \"escalations\": {},\n  \
         \"tokens\": {},\n  \
         \"input_tokens\": {},\n  \
         \"output_tokens\": {},\n  \
         \"wall_ms\": {},\n  \
         \"p50_wall_ms\": {},\n  \
         \"no_tool_tries\": {},\n  \
         \"sources\": {}\n}}\n",
        json_escape(totals.model),
        json_escape(totals.provider),
        totals.task_set,
        totals.tries,
        totals.passed,
        pass_rate(totals.passed, totals.tries),
        totals.escalations,
        totals.input_tokens.saturating_add(totals.output_tokens),
        totals.input_tokens,
        totals.output_tokens,
        totals.wall_ms,
        totals.p50_wall_ms,
        totals.no_tool_tries,
        sources_json(),
    )
}

fn sources_json() -> String {
    let mut out = String::from("[\n");
    for (i, fix) in fixtures().iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!(
            "    {{\"id\": \"{}\", \"source\": \"{}\"}}",
            json_escape(fix.id),
            json_escape(fix.source)
        ));
    }
    out.push_str("\n  ]");
    out
}

fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `<out>.results.json` beside the markdown report.
pub fn results_path(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".results.json");
    PathBuf::from(name)
}

const FIXTURES: [CodingFixture; 30] = [
    CodingFixture {
        id: "code-chat-preview",
        source: "GrokHub#2146",
        turns: ASK,
        bug: "pub fn chat_preview(turns: &[&str]) -> String {\n    turns.join(\" \")\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn first_line_only() {\n    assert_eq!(chat_preview(&[\"Hello there\\nmore\", \"second\"]), \"Hello there\");\n}\n",
    },
    CodingFixture {
        id: "code-manifest-alias",
        source: "GrokHub#2165",
        turns: ASK,
        bug: "pub fn manifest_name_ok(asset: &str, alias: &str) -> bool {\n    asset == alias\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn alias_matches_the_arch_build() {\n    assert!(manifest_name_ok(\"GrokHub-Shell-x86_64.AppImage\", \"GrokHub-Shell\"));\n    assert!(manifest_name_ok(\"GrokHub-Shell.AppImage\", \"GrokHub-Shell\"));\n    assert!(!manifest_name_ok(\"Other.AppImage\", \"GrokHub-Shell\"));\n}\n",
    },
    CodingFixture {
        id: "code-plain-values",
        source: "GrokHub#2138",
        turns: ASK,
        bug: "pub fn values(tokens: &[(&str, &str)]) -> Vec<String> {\n    tokens.iter().filter(|(_, value)| !value.starts_with('-')).map(|(_, value)| (*value).to_string()).collect()\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_value_that_looks_like_a_flag_stays() {\n    assert_eq!(values(&[(\"n\", \"-1\"), (\"name\", \"bolt\")]), vec![\"-1\".to_string(), \"bolt\".to_string()]);\n}\n",
    },
    CodingFixture {
        id: "code-runs-git",
        source: "GrokHub#2188",
        turns: ASK,
        bug: "pub fn runs_git(argv: &[&str]) -> bool {\n    argv.first().copied() == Some(\"git\")\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn git_counts_however_it_is_launched() {\n    assert!(runs_git(&[\"bash\", \"-lc\", \"git push origin beta\"]));\n    assert!(runs_git(&[\"/usr/bin/git\", \"status\"]));\n    assert!(!runs_git(&[\"echo\", \"git\"]));\n    assert!(!runs_git(&[\"echo\", \"hello\"]));\n}\n",
    },
    CodingFixture {
        id: "code-elicit-not-approval",
        source: "GrokHub#2190",
        turns: ASK,
        bug: "pub fn counts_as_approval(kind: &str) -> bool {\n    kind == \"tool\" || kind == \"elicit\"\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn an_elicitation_is_not_an_approval() {\n    assert!(!counts_as_approval(\"elicit\"));\n    assert!(counts_as_approval(\"tool\"));\n    assert!(!counts_as_approval(\"chat\"));\n}\n",
    },
    CodingFixture {
        id: "code-browser-stays-out",
        source: "GrokHub#2187",
        turns: ASK,
        bug: "pub fn desktop_instruction<'a>(browser_text: &'a str, user_text: &'a str) -> &'a str {\n    if browser_text.is_empty() { user_text } else { browser_text }\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn desktop_control_uses_the_typed_instruction() {\n    assert_eq!(desktop_instruction(\"click buy\", \"scroll the page\"), \"scroll the page\");\n}\n",
    },
    CodingFixture {
        id: "code-real-remove",
        source: "GrokHub#2168",
        turns: ASK,
        bug: "pub fn is_remove(argv: &[&str]) -> bool {\n    argv.iter().any(|arg| arg.contains(\"rm\"))\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn only_a_real_rm_command_matches() {\n    assert!(!is_remove(&[\"firmware\", \"check\"]));\n    assert!(is_remove(&[\"rm\", \"build\"]));\n    assert!(is_remove(&[\"/usr/bin/rm\", \"build\"]));\n}\n",
    },
    CodingFixture {
        id: "code-masked-pin",
        source: "GrokHub#2196",
        turns: ASK,
        bug: "pub fn pin_ok(expected: &str, got: &str) -> bool {\n    expected == got || got.contains('*')\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_masked_hash_is_not_the_pin() {\n    assert!(!pin_ok(\"abcd1234\", \"abcd****\"));\n    assert!(pin_ok(\"abcd1234\", \"abcd1234\"));\n    assert!(!pin_ok(\"abcd1234\", \"abcd1235\"));\n}\n",
    },
    CodingFixture {
        id: "code-cert-pin",
        source: "GrokHub#2194",
        turns: ASK,
        bug: "pub fn cert_ok(_pinned: &str, presented: &str) -> bool {\n    !presented.is_empty()\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn only_the_pinned_cert_is_accepted() {\n    assert!(!cert_ok(\"sha256-aa\", \"sha256-bb\"));\n    assert!(cert_ok(\"sha256-aa\", \"sha256-aa\"));\n    assert!(!cert_ok(\"sha256-aa\", \"\"));\n}\n",
    },
    CodingFixture {
        id: "code-typed-git",
        source: "GrokHub#2183",
        turns: ASK,
        bug: "pub fn git_fast_path(_typed_by_user: bool, text: &str) -> bool {\n    text.split_whitespace().any(|word| word == \"git\")\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn the_fast_path_needs_the_person_to_have_typed_it() {\n    assert!(!git_fast_path(false, \"git status\"));\n    assert!(git_fast_path(true, \"git status\"));\n    assert!(!git_fast_path(true, \"status\"));\n}\n",
    },
    CodingFixture {
        id: "code-offer-missing",
        source: "GrokHub#2172",
        turns: ASK,
        bug: "pub fn should_offer(_installed: bool, better: bool) -> bool {\n    better\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn an_installed_model_is_not_offered_again() {\n    assert!(!should_offer(true, true));\n    assert!(should_offer(false, true));\n    assert!(!should_offer(false, false));\n}\n",
    },
    CodingFixture {
        id: "code-save-flip",
        source: "GrokHub#2161",
        turns: ASK,
        bug: "pub fn commit_toggle(currently_on: bool) -> bool {\n    currently_on\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn the_saved_value_is_the_state_after_the_flip() {\n    assert!(!commit_toggle(true));\n    assert!(commit_toggle(false));\n}\n",
    },
    CodingFixture {
        id: "code-mic-opt-in",
        source: "GrokHub#2158",
        turns: ASK,
        bug: "pub fn mic_open(_opt_in: bool, listening: bool) -> bool {\n    listening\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn the_mic_stays_closed_until_opt_in() {\n    assert!(!mic_open(false, true));\n    assert!(mic_open(true, true));\n    assert!(!mic_open(true, false));\n}\n",
    },
    CodingFixture {
        id: "code-sealed-grant",
        source: "GrokHub#2139",
        turns: ASK,
        bug: "pub fn grant_applies(grant_id: &str, _routine_id: &str) -> bool {\n    !grant_id.is_empty()\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_grant_applies_only_to_its_own_routine() {\n    assert!(!grant_applies(\"grant-a\", \"routine-b\"));\n    assert!(grant_applies(\"routine-a\", \"routine-a\"));\n    assert!(!grant_applies(\"\", \"\"));\n}\n",
    },
    CodingFixture {
        id: "code-task-ready",
        source: "GrokHub#2197",
        turns: ASK,
        bug: "pub fn ready(status: &str) -> bool {\n    status == \"running\" || status == \"done\"\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_running_task_is_not_ready() {\n    assert!(!ready(\"running\"));\n    assert!(ready(\"done\"));\n    assert!(!ready(\"failed\"));\n}\n",
    },
    CodingFixture {
        id: "code-read-only-ask",
        source: "GrokHub#2195",
        turns: ASK,
        bug: "pub fn tool_allowed(_read_only_ask: bool, _tool: &str) -> bool {\n    true\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_read_only_question_cannot_write() {\n    assert!(!tool_allowed(true, \"write_file\"));\n    assert!(tool_allowed(true, \"read_file\"));\n    assert!(tool_allowed(false, \"write_file\"));\n}\n",
    },
    CodingFixture {
        id: "code-agent-root",
        source: "GrokHub#2180",
        turns: ASK,
        bug: "pub fn shell_root<'a>(_agent_root: &'a str, parent_root: &'a str) -> &'a str {\n    parent_root\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn the_shell_stays_in_the_agent_workspace() {\n    assert_eq!(shell_root(\"/home/user/work/agent\", \"/home/user/work\"), \"/home/user/work/agent\");\n}\n",
    },
    CodingFixture {
        id: "code-route-ask",
        source: "GrokHub#2170",
        turns: ASK,
        bug: "pub fn route_key<'a>(wrapper: &'a str, _ask: &'a str) -> &'a str {\n    wrapper\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn routing_uses_the_ask_when_there_is_one() {\n    assert_eq!(route_key(\"chat\", \"fix the test\"), \"fix the test\");\n    assert_eq!(route_key(\"chat\", \"\"), \"chat\");\n}\n",
    },
    CodingFixture {
        id: "code-composer-height",
        source: "GrokHub#2159",
        turns: ASK,
        bug: "pub fn composer_height(_window_h: i32, natural: i32) -> i32 {\n    natural\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn the_composer_fits_the_opening_window() {\n    assert_eq!(composer_height(300, 400), 100);\n    assert_eq!(composer_height(900, 40), 40);\n}\n",
    },
    CodingFixture {
        id: "code-fetch-https",
        source: "GrokHub#2178",
        turns: ASK,
        bug: "pub fn fetch_ok(url: &str) -> bool {\n    url.contains(\"https://\")\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn fetch_allows_only_a_plain_https_url() {\n    assert!(fetch_ok(\"https://example.com/a\"));\n    assert!(!fetch_ok(\"note https://example.com\"));\n    assert!(!fetch_ok(\"https://example.com/file://secret\"));\n}\n",
    },
    CodingFixture {
        id: "code-update-label",
        source: "tech-bench#13",
        turns: ASK,
        bug: "pub fn update_label(check_ok: bool, same_version: bool) -> &'static str {\n    if same_version || !check_ok { \"current\" } else { \"update\" }\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_failed_check_is_not_reported_as_current() {\n    assert_eq!(update_label(false, true), \"unknown\");\n    assert_eq!(update_label(true, true), \"current\");\n    assert_eq!(update_label(true, false), \"update\");\n}\n",
    },
    CodingFixture {
        id: "code-short-frame",
        source: "tech-bench#1",
        turns: ASK,
        bug: "pub fn frame_kept(dlc: u8, len: usize) -> bool {\n    dlc == 8 && len == 8\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_short_frame_with_its_bytes_is_kept() {\n    assert!(frame_kept(3, 3));\n    assert!(frame_kept(8, 8));\n    assert!(!frame_kept(8, 3));\n    assert!(!frame_kept(0, 0));\n}\n",
    },
    CodingFixture {
        id: "code-search-rank",
        source: "tech-bench#1",
        turns: ASK,
        bug: "pub fn score(_hits: usize, title_len: usize) -> i32 {\n    title_len as i32\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn more_hits_rank_above_a_longer_title() {\n    assert!(score(3, 10) > score(1, 100));\n}\n",
    },
    CodingFixture {
        id: "code-fresh-hud",
        source: "tech-bench#4",
        turns: ASK,
        bug: "pub fn hud(previous: i32, _fresh: Option<i32>) -> i32 {\n    previous\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn a_fresh_reading_replaces_the_stale_one() {\n    assert_eq!(hud(10, Some(4)), 4);\n    assert_eq!(hud(10, None), 10);\n}\n",
    },
    CodingFixture {
        id: "code-keep-notes",
        source: "tech-bench#4",
        turns: ASK,
        bug: "pub fn after_clear(_notes: &str, _codes: &[u32]) -> (String, Vec<u32>) {\n    (String::new(), Vec::new())\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn clearing_codes_keeps_the_note() {\n    let (notes, codes) = after_clear(\"user note\", &[1, 2]);\n    assert_eq!(notes, \"user note\");\n    assert!(codes.is_empty());\n}\n",
    },
    CodingFixture {
        id: "code-merge-orders",
        source: "tech-bench#17",
        turns: ASK,
        bug: "pub fn merge(_local: &[u32], remote: &[u32]) -> Vec<u32> {\n    remote.to_vec()\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn merge_keeps_ids_from_both_sides() {\n    assert_eq!(merge(&[2, 1], &[2, 3]), vec![1, 2, 3]);\n}\n",
    },
    CodingFixture {
        id: "code-drop-secret",
        source: "tech-bench#17",
        turns: ASK,
        bug: "pub fn packet(id: &str, note: &str, secret: &str) -> String {\n    format!(\"{id}|{note}|{secret}\")\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn the_packet_leaves_the_secret_out() {\n    let body = packet(\"WO-1\", \"hose\", \"sk-abcdefghijklmnopqrstuv\");\n    assert!(!body.contains(\"sk-abcdefghijklmnopqrstuv\"));\n    assert!(body.contains(\"WO-1\"));\n    assert!(body.contains(\"hose\"));\n}\n",
    },
    CodingFixture {
        id: "code-wrap-text",
        source: "tech-bench#11",
        turns: ASK,
        bug: "pub fn fit(text: &str, width: usize) -> String {\n    text.chars().take(width).collect()\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn text_wraps_instead_of_clipping() {\n    assert_eq!(fit(\"abcdef\", 3), \"abc\\ndef\");\n    assert_eq!(fit(\"ab\", 3), \"ab\");\n}\n",
    },
    CodingFixture {
        id: "code-reset-dest",
        source: "tech-bench#10",
        turns: ASK,
        bug: "pub fn reset_dest(_kind: &str) -> u8 {\n    0\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn an_engine_or_compressor_reset_goes_to_address_48() {\n    assert_eq!(reset_dest(\"compressor\"), 48);\n    assert_eq!(reset_dest(\"engine\"), 48);\n    assert_eq!(reset_dest(\"cab\"), 0);\n}\n",
    },
    CodingFixture {
        id: "code-def-level",
        source: "tech-bench#12",
        turns: ASK,
        bug: "pub fn def_percent(raw: u8) -> u8 {\n    raw / 2\n}\n",
        tests: "#[path = \"../src/bug.rs\"]\nmod bug;\nuse bug::*;\n\n#[test]\nfn def_level_is_tenths_of_a_percent_times_four() {\n    assert_eq!(def_percent(0), 0);\n    assert_eq!(def_percent(100), 40);\n    assert_eq!(def_percent(250), 100);\n}\n",
    },
];

#[cfg(test)]
fn known_good(id: &str) -> &'static str {
    match id {
        "code-chat-preview" => "pub fn chat_preview(turns: &[&str]) -> String {\n    let first = turns.first().copied().unwrap_or(\"\");\n    let line = first.lines().next().unwrap_or(\"\");\n    line.chars().take(80).collect()\n}\n",
        "code-manifest-alias" => "pub fn manifest_name_ok(asset: &str, alias: &str) -> bool {\n    let stem = asset.strip_suffix(\".AppImage\").unwrap_or(asset);\n    stem == alias || stem.starts_with(&format!(\"{alias}-\"))\n}\n",
        "code-plain-values" => "pub fn values(tokens: &[(&str, &str)]) -> Vec<String> {\n    tokens.iter().map(|(_, value)| (*value).to_string()).collect()\n}\n",
        "code-runs-git" => "fn is_git(word: &str) -> bool {\n    let base = word.rsplit(['/', '\\\\']).next().unwrap_or(word);\n    base == \"git\"\n}\n\npub fn runs_git(argv: &[&str]) -> bool {\n    if argv.first().copied().is_some_and(is_git) {\n        return true;\n    }\n    let shell = argv.first().copied().is_some_and(|arg| {\n        let base = arg.rsplit(['/', '\\\\']).next().unwrap_or(arg);\n        matches!(base, \"bash\" | \"sh\" | \"zsh\" | \"pwsh\" | \"powershell\" | \"cmd\" | \"cmd.exe\")\n    });\n    shell && argv.iter().skip(1).any(|arg| arg.split_whitespace().any(is_git))\n}\n",
        "code-elicit-not-approval" => "pub fn counts_as_approval(kind: &str) -> bool {\n    kind == \"tool\"\n}\n",
        "code-browser-stays-out" => "pub fn desktop_instruction<'a>(_browser_text: &'a str, user_text: &'a str) -> &'a str {\n    user_text\n}\n",
        "code-real-remove" => "pub fn is_remove(argv: &[&str]) -> bool {\n    argv.iter().any(|arg| {\n        let base = arg.rsplit(['/', '\\\\']).next().unwrap_or(arg);\n        base == \"rm\" || base == \"rm.exe\"\n    })\n}\n",
        "code-masked-pin" => "pub fn pin_ok(expected: &str, got: &str) -> bool {\n    !expected.is_empty() && !expected.contains('*') && !got.contains('*') && expected == got\n}\n",
        "code-cert-pin" => "pub fn cert_ok(pinned: &str, presented: &str) -> bool {\n    !pinned.is_empty() && pinned == presented\n}\n",
        "code-typed-git" => "pub fn git_fast_path(typed_by_user: bool, text: &str) -> bool {\n    typed_by_user && text.split_whitespace().any(|word| word == \"git\")\n}\n",
        "code-offer-missing" => "pub fn should_offer(installed: bool, better: bool) -> bool {\n    better && !installed\n}\n",
        "code-save-flip" => "pub fn commit_toggle(currently_on: bool) -> bool {\n    !currently_on\n}\n",
        "code-mic-opt-in" => "pub fn mic_open(opt_in: bool, listening: bool) -> bool {\n    opt_in && listening\n}\n",
        "code-sealed-grant" => "pub fn grant_applies(grant_id: &str, routine_id: &str) -> bool {\n    !grant_id.is_empty() && grant_id == routine_id\n}\n",
        "code-task-ready" => "pub fn ready(status: &str) -> bool {\n    status == \"done\"\n}\n",
        "code-read-only-ask" => "pub fn tool_allowed(read_only_ask: bool, tool: &str) -> bool {\n    if read_only_ask {\n        matches!(tool, \"read_file\" | \"grep\" | \"list_dir\")\n    } else {\n        true\n    }\n}\n",
        "code-agent-root" => "pub fn shell_root<'a>(agent_root: &'a str, _parent_root: &'a str) -> &'a str {\n    agent_root\n}\n",
        "code-route-ask" => "pub fn route_key<'a>(wrapper: &'a str, ask: &'a str) -> &'a str {\n    if ask.is_empty() { wrapper } else { ask }\n}\n",
        "code-composer-height" => "pub fn composer_height(window_h: i32, natural: i32) -> i32 {\n    let cap = (window_h / 3).max(32);\n    natural.clamp(32, cap)\n}\n",
        "code-fetch-https" => "pub fn fetch_ok(url: &str) -> bool {\n    url.starts_with(\"https://\") && !url.to_ascii_lowercase().contains(\"file:\")\n}\n",
        "code-update-label" => "pub fn update_label(check_ok: bool, same_version: bool) -> &'static str {\n    if !check_ok { \"unknown\" } else if same_version { \"current\" } else { \"update\" }\n}\n",
        "code-short-frame" => "pub fn frame_kept(dlc: u8, len: usize) -> bool {\n    len > 0 && len <= 8 && dlc as usize == len\n}\n",
        "code-search-rank" => "pub fn score(hits: usize, _title_len: usize) -> i32 {\n    hits as i32\n}\n",
        "code-fresh-hud" => "pub fn hud(previous: i32, fresh: Option<i32>) -> i32 {\n    fresh.unwrap_or(previous)\n}\n",
        "code-keep-notes" => "pub fn after_clear(notes: &str, _codes: &[u32]) -> (String, Vec<u32>) {\n    (notes.to_string(), Vec::new())\n}\n",
        "code-merge-orders" => "pub fn merge(local: &[u32], remote: &[u32]) -> Vec<u32> {\n    let mut all = local.to_vec();\n    for id in remote {\n        if !all.contains(id) {\n            all.push(*id);\n        }\n    }\n    all.sort_unstable();\n    all\n}\n",
        "code-drop-secret" => "pub fn packet(id: &str, note: &str, _secret: &str) -> String {\n    format!(\"{id}|{note}\")\n}\n",
        "code-wrap-text" => "pub fn fit(text: &str, width: usize) -> String {\n    if width == 0 {\n        return String::new();\n    }\n    let chars: Vec<char> = text.chars().collect();\n    let mut out = String::new();\n    for (i, ch) in chars.iter().enumerate() {\n        if i > 0 && i % width == 0 {\n            out.push('\\n');\n        }\n        out.push(*ch);\n    }\n    out\n}\n",
        "code-reset-dest" => "pub fn reset_dest(kind: &str) -> u8 {\n    match kind {\n        \"engine\" | \"compressor\" => 48,\n        _ => 0,\n    }\n}\n",
        "code-def-level" => "pub fn def_percent(raw: u8) -> u8 {\n    let pct = (raw as u16) * 2 / 5;\n    pct.min(100) as u8\n}\n",
        _ => panic!("no known-good file for {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_tasks_from_both_repos_with_unique_ids() {
        let all = fixtures();
        assert_eq!(all.len(), 30);
        let mut ids: Vec<&str> = all.iter().map(|fix| fix.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 30);
        assert!(ids.iter().all(|id| id.starts_with("code-")));
        assert_eq!(
            all.iter()
                .filter(|fix| fix.source.starts_with("GrokHub#"))
                .count(),
            20
        );
        assert_eq!(
            all.iter()
                .filter(|fix| fix.source.starts_with("tech-bench#"))
                .count(),
            10
        );
        for fix in all {
            assert!(fix.tests.contains("#[test]"), "{}", fix.id);
            assert_ne!(fix.bug, super::known_good(fix.id), "{}", fix.id);
            assert!(
                !fix.bug.contains("/home/") || fix.bug.contains("/home/user"),
                "{}",
                fix.id
            );
        }
    }

    #[test]
    fn the_seeded_bug_fails_and_the_known_fix_passes_and_a_wiped_test_is_restored() {
        let root = std::env::temp_dir().join(format!("gh-coding-oracle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for fix in fixtures() {
            let ws = root.join(fix.id);
            let _ = std::fs::remove_dir_all(&ws);
            seed_workspace(&ws, fix).unwrap();
            let failed = grade(&ws, fix);
            assert!(failed.is_err(), "{} seeded bug passed: {failed:?}", fix.id);
            std::fs::write(ws.join("src/bug.rs"), super::known_good(fix.id)).unwrap();
            std::fs::write(ws.join("tests/check.rs"), "// loosened\n").unwrap();
            grade(&ws, fix).unwrap_or_else(|err| panic!("{} known fix failed: {err}", fix.id));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn escalations_count_model_switches_and_the_results_name_the_model() {
        assert_eq!(count_escalations(&[vec!["local-a", "local-a"]]), 0);
        assert_eq!(
            count_escalations(&[vec!["local-a", "grok-4"], vec!["local-a"]]),
            1
        );
        let totals = CodingTotals {
            model: "local-a",
            provider: "local",
            task_set: 30,
            tries: 2,
            passed: 1,
            escalations: 1,
            input_tokens: 10,
            output_tokens: 4,
            wall_ms: 90,
            p50_wall_ms: 40,
            no_tool_tries: 0,
        };
        let json = results_json(&totals);
        assert!(json.contains("\"model\": \"local-a\""), "{json}");
        assert!(json.contains("\"provider\": \"local\""), "{json}");
        assert!(json.contains("\"pass_rate\": \"50% (1/2)\""), "{json}");
        assert!(json.contains("\"escalations\": 1"), "{json}");
        assert!(json.contains("\"tokens\": 14"), "{json}");
        assert!(json.contains("\"wall_ms\": 90"), "{json}");
        assert!(json.contains("\"task_set\": 30"), "{json}");
        assert!(json.contains("\"source\": \"GrokHub#2146\""), "{json}");
        assert!(json.contains("\"source\": \"tech-bench#13\""), "{json}");
        let md = render_section(&totals);
        assert!(
            md.contains("| local-a | local | 50% (1/2) | 1 | 14 | 10 | 4 | 90 | 40 | 0 |"),
            "{md}"
        );
        let path = results_path(Path::new("coding-local.md"));
        assert_eq!(path, PathBuf::from("coding-local.md.results.json"));
    }

    #[test]
    fn a_run_where_no_try_made_a_tool_call_says_it_measured_the_server() {
        let totals = |no_tool_tries| CodingTotals {
            model: "local-a",
            provider: "local",
            task_set: 30,
            tries: 30,
            passed: 0,
            escalations: 0,
            input_tokens: 10,
            output_tokens: 4,
            wall_ms: 90,
            p50_wall_ms: 3,
            no_tool_tries,
        };
        let md = render_section(&totals(30));
        assert!(md.contains("| 3 | 30 |"), "{md}");
        assert!(
            md.contains("measures the model server, not the model"),
            "{md}"
        );
        assert!(results_json(&totals(30)).contains("\"no_tool_tries\": 30"));
        assert!(!render_section(&totals(29)).contains("model server"));
    }
}
