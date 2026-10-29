//! Care-first arm against today's chat route, on a fixed fixture set.
//!
//! There is no live quality judge in this harness. [`CARE_FIRST_SHIPS_ON`]
//! stays false, and the shipped `care_first` switch stays false with it.
//! Cost, words and latency here are offline proxies. The scripted p95 is the
//! fake client's return time and does not clear a live latency budget.

use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;

use grokhub_core::brain_policy::{apply_care_first, builtin_policy, pick_row, PolicyRow};
use grokhub_core::model_registry::EFFORT_LADDER;
use grokhub_core::talk::TalkStyle;

use crate::brain::{classify_effort, rules, Brief, BriefInput};
use crate::client::{
    CancelToken, ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent,
    TurnOutput,
};
use crate::route::difficulty::{self, DifficultyInput};
use crate::route::policy::{self, predicted_tokens};
use crate::route::{goal, ladder};

use super::metrics::{estimate_tokens, offline_cost_ticks};
use super::sycophancy;

/// Card budgets: quality must rise, cost at most +25%, p95 at most +20%,
/// words at most +40%. Missing a live quality score means this stays false.
pub const CARE_FIRST_SHIPS_ON: bool = false;

const FIXTURE_DIR: &str = "tests/fixtures/care_first";
const CHAT_CLASS: &str = "chat:default";
const ROUNDS: usize = 5;
const COST_BUDGET: f64 = 1.25;
const P95_BUDGET: f64 = 1.20;
const WORDS_BUDGET: f64 = 1.40;

/// True only when a measured arm beats every card budget at once.
pub fn care_first_default_on(
    quality_up: bool,
    cost_ratio: f64,
    p95_ratio: f64,
    words_ratio: f64,
) -> bool {
    quality_up
        && cost_ratio <= COST_BUDGET
        && p95_ratio <= P95_BUDGET
        && words_ratio <= WORDS_BUDGET
}

#[derive(Debug, Clone, PartialEq)]
pub struct CareFirstTurn {
    pub id: String,
    pub kind: String,
    pub prompt: String,
    pub baseline_effort: String,
    pub care_effort: String,
    pub baseline_goal: String,
    pub care_goal: String,
    pub baseline_talk: String,
    pub care_talk: String,
    pub words: u64,
    pub baseline_cost_ticks: i64,
    pub care_cost_ticks: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CareFirstReport {
    pub turns: Vec<CareFirstTurn>,
    pub baseline_cost_ticks: i64,
    pub care_cost_ticks: i64,
    pub cost_ratio: f64,
    pub baseline_p50_ms: f64,
    pub care_p50_ms: f64,
    pub baseline_p95_ms: f64,
    pub care_p95_ms: f64,
    pub p95_ratio: f64,
    pub baseline_words: u64,
    pub care_words: u64,
    pub words_ratio: f64,
    pub baseline_sycophancy: f64,
    pub care_sycophancy: f64,
    /// Empty offline. A live judge would put a quality delta here.
    pub quality_judge: Option<f64>,
    pub classify_effort: &'static str,
    pub care_classify_effort: &'static str,
    pub ships_on: bool,
    pub why: String,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    id: String,
    kind: String,
    prompt: String,
    reply: String,
}

struct Routed {
    effort: String,
    goal: String,
    talk: String,
    block: String,
}

/// Run the care-first arm and today's route on the fixed fixtures.
pub fn run_care_first_ab() -> Result<CareFirstReport, String> {
    let fixtures = load_fixtures()?;
    let baseline_rows = builtin_policy();
    let care_rows = apply_care_first(baseline_rows.clone());
    let mut turns = Vec::with_capacity(fixtures.len());
    let mut base_lat = Vec::new();
    let mut care_lat = Vec::new();
    for fixture in &fixtures {
        let brief = brief_for(&fixture.prompt);
        let base = route_turn(&fixture.prompt, &brief, &baseline_rows, false);
        let care = route_turn(&fixture.prompt, &brief, &care_rows, true);
        let words = word_count(&fixture.reply);
        let baseline_cost_ticks = turn_cost(&base.effort, &base.block, &fixture.reply)?;
        let care_cost_ticks = turn_cost(&care.effort, &care.block, &fixture.reply)?;
        for round in 0..ROUNDS {
            let mut order = [(false, base.block.as_str()), (true, care.block.as_str())];
            if round % 2 == 1 {
                order.swap(0, 1);
            }
            for (is_care, block) in order {
                let ms = sample_ms(block, &fixture.reply)?;
                if is_care {
                    care_lat.push(ms);
                } else {
                    base_lat.push(ms);
                }
            }
        }
        turns.push(CareFirstTurn {
            id: fixture.id.clone(),
            kind: fixture.kind.clone(),
            prompt: fixture.prompt.clone(),
            baseline_effort: base.effort,
            care_effort: care.effort,
            baseline_goal: base.goal,
            care_goal: care.goal,
            baseline_talk: base.talk,
            care_talk: care.talk,
            words,
            baseline_cost_ticks,
            care_cost_ticks,
        });
    }
    let baseline_cost_ticks = turns.iter().map(|t| t.baseline_cost_ticks).sum();
    let care_cost_ticks = turns.iter().map(|t| t.care_cost_ticks).sum();
    let baseline_words = turns.iter().map(|t| t.words).sum();
    let care_words = baseline_words;
    let cost_ratio = ratio(care_cost_ticks as f64, baseline_cost_ticks as f64);
    let words_ratio = ratio(care_words as f64, baseline_words as f64);
    base_lat.sort_by(f64::total_cmp);
    care_lat.sort_by(f64::total_cmp);
    let baseline_p50_ms = percentile(&base_lat, 0.50);
    let care_p50_ms = percentile(&care_lat, 0.50);
    let baseline_p95_ms = percentile(&base_lat, 0.95);
    let care_p95_ms = percentile(&care_lat, 0.95);
    let p95_ratio = ratio(care_p95_ms, baseline_p95_ms);
    let baseline_skill = TalkStyle::Short.block();
    let care_skill = TalkStyle::Natural.prompt_block(true);
    sycophancy::gate(&baseline_skill).map_err(|err| format!("baseline sycophancy: {err}"))?;
    sycophancy::gate(&care_skill).map_err(|err| format!("care-first sycophancy: {err}"))?;
    let baseline_sycophancy = agreement_rate(&sycophancy::report(&baseline_skill))?;
    let care_sycophancy = agreement_rate(&sycophancy::report(&care_skill))?;
    let classify = classify_effort(false);
    let care_classify = classify_effort(true);
    let quality_judge = None;
    let ships_on = care_first_default_on(false, cost_ratio, p95_ratio, words_ratio);
    let why = format!(
        "Default stays off. Skipped: live quality judge, so quality did not rise. \
Cost proxy {care_cost_ticks}/{baseline_cost_ticks} ticks (ratio {cost_ratio:.3}) counts \
chat:default predicted input plus the style block, fixture-reply output, and a fixed \
reasoning table for the rung. It is not a live bill. Words {care_words}/{baseline_words} \
(ratio {words_ratio:.3}) are the same fixture replies on both arms, so they do not show \
a live verbosity change. Skipped: live model latency. Scripted p50 {care_p50_ms:.3} ms vs \
{baseline_p50_ms:.3} ms and p95 {care_p95_ms:.3} ms vs {baseline_p95_ms:.3} ms (ratio \
{p95_ratio:.3}) time a fake client return and do not clear the live p95 budget. \
Sycophancy agreement with profile is {care_sycophancy:.1} on the soft natural block and \
{baseline_sycophancy:.1} on the short block. Classify effort stays {classify} with \
the switch off and {care_classify} with it on. The cost and word proxies sit inside \
their budgets, and that is not a quality gain."
    );
    Ok(CareFirstReport {
        turns,
        baseline_cost_ticks,
        care_cost_ticks,
        cost_ratio,
        baseline_p50_ms,
        care_p50_ms,
        baseline_p95_ms,
        care_p95_ms,
        p95_ratio,
        baseline_words,
        care_words,
        words_ratio,
        baseline_sycophancy,
        care_sycophancy,
        quality_judge,
        classify_effort: classify,
        care_classify_effort: care_classify,
        ships_on,
        why,
    })
}

fn load_fixtures() -> Result<Vec<Fixture>, String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR);
    let entries =
        std::fs::read_dir(&dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    let mut fixtures = Vec::new();
    for entry in entries {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        let fixture: Fixture =
            serde_json::from_str(&raw).map_err(|err| format!("parse {}: {err}", path.display()))?;
        fixtures.push(fixture);
    }
    fixtures.sort_by(|a, b| a.id.cmp(&b.id));
    if fixtures.len() != 6 {
        return Err(format!(
            "expected 6 care-first fixtures, got {}",
            fixtures.len()
        ));
    }
    Ok(fixtures)
}

fn brief_for(text: &str) -> Brief {
    let input = BriefInput {
        text,
        files: 0,
        tools: &[],
        recent: &[],
        background: false,
    };
    rules(&input).unwrap_or_else(|| Brief {
        mood: "calm".into(),
        domain: "general".into(),
        by: "default".into(),
        ..Brief::default()
    })
}

fn route_turn(text: &str, brief: &Brief, rows: &[PolicyRow], care_first: bool) -> Routed {
    let row =
        pick_row(rows, brief.intent.as_str(), brief.care.as_str()).expect("a policy row matches");
    let class = policy::class_row(CHAT_CLASS).expect("chat:default");
    let band = ladder::Band::of(class, 0);
    let d = difficulty::difficulty(&DifficultyInput {
        text,
        ..DifficultyInput::default()
    });
    let steer = ladder::steer_for_turn(text, care_first);
    let hold = ladder::hold_chat_drops(class.class, care_first);
    let user = ladder::user_facing(class.class);
    let goal = goal::RouteGoal::of(&row.goal, &row.reason);
    let start = goal::goal_start(
        band,
        ladder::start_holding(band, d, steer, user, hold),
        steer,
        goal.as_ref().filter(|_| user),
    );
    let pick = ladder::advance_holding(
        None,
        class.class,
        band,
        start,
        &ladder::Obs {
            new_turn: true,
            ..ladder::Obs::default()
        },
        hold,
    )
    .1;
    let style = TalkStyle::of(&row.talk);
    Routed {
        effort: EFFORT_LADDER[pick.rung].to_string(),
        goal: row.goal.clone(),
        talk: style.label(),
        block: style.prompt_block(care_first),
    }
}

/// Offline ticks for one chat turn. Input is the class prediction plus the
/// style block. Cached share covers the prediction only. Output is the
/// fixture reply. Reasoning is a fixed table for the rung, not a tokenizer.
fn turn_cost(effort: &str, block: &str, reply: &str) -> Result<i64, String> {
    let (input, _predicted_out, cache_pct) = predicted_tokens(CHAT_CLASS);
    let cached = input.saturating_mul(cache_pct) / 100;
    let style = estimate_tokens(block.len() as u64);
    let output = estimate_tokens(reply.len() as u64);
    let reasoning = reasoning_tokens(effort)?;
    Ok(offline_cost_ticks(
        input.saturating_add(style),
        output,
        reasoning,
        cached,
    ))
}

fn reasoning_tokens(effort: &str) -> Result<u64, String> {
    Ok(match effort {
        "none" => 0,
        "minimal" => 8,
        "low" => 20,
        "medium" => 80,
        "high" => 200,
        "xhigh" => 400,
        "max" => 800,
        other => return Err(format!("unknown effort {other}")),
    })
}

fn word_count(text: &str) -> u64 {
    text.split_whitespace().count() as u64
}

fn ratio(num: f64, den: f64) -> f64 {
    if den == 0.0 {
        if num == 0.0 {
            1.0
        } else {
            f64::INFINITY
        }
    } else {
        num / den
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn agreement_rate(report: &str) -> Result<f64, String> {
    let rest = report
        .split("with profile ")
        .nth(1)
        .ok_or_else(|| format!("sycophancy report has no with-profile rate: {report}"))?;
    let token = rest
        .split(';')
        .next()
        .unwrap_or(rest)
        .split_whitespace()
        .next()
        .unwrap_or("");
    let mut parts = token.split('/');
    let hits: f64 = parts
        .next()
        .unwrap_or("1")
        .parse()
        .map_err(|_| format!("bad sycophancy hits in {token}"))?;
    let n: f64 = parts
        .next()
        .unwrap_or("0")
        .parse()
        .map_err(|_| format!("bad sycophancy n in {token}"))?;
    if n == 0.0 {
        return Err(format!("sycophancy rate has no turns: {token}"));
    }
    Ok(hits / n)
}

struct Scripted(String);

impl ModelClient for Scripted {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        sink(StreamEvent::TextDelta(self.0.clone()));
        Ok(TurnOutput {
            text: self.0.clone(),
            ..TurnOutput::default()
        })
    }
}

/// Text the dry-run suite keeps so this arm stays on the eval path.
pub fn status_line(report: &CareFirstReport) -> String {
    let quality = match report.quality_judge {
        Some(score) => score.to_string(),
        None => "skipped".to_string(),
    };
    let mut out = format!(
        "care-first ships {} const {} cost {}/{} ratio {:.3} p50 {:.3}/{:.3} p95 {:.3}/{:.3} ratio {:.3} words {}/{} ratio {:.3} sycophancy {:.1}/{:.1} classify {}/{} quality {quality}. {}",
        report.ships_on,
        CARE_FIRST_SHIPS_ON,
        report.care_cost_ticks,
        report.baseline_cost_ticks,
        report.cost_ratio,
        report.care_p50_ms,
        report.baseline_p50_ms,
        report.care_p95_ms,
        report.baseline_p95_ms,
        report.p95_ratio,
        report.care_words,
        report.baseline_words,
        report.words_ratio,
        report.care_sycophancy,
        report.baseline_sycophancy,
        report.care_classify_effort,
        report.classify_effort,
        report.why
    );
    for turn in &report.turns {
        out.push('\n');
        out.push_str(&format!(
            "{} {} {} {}->{} {}->{} {}->{} words {} cost {}/{}",
            turn.id,
            turn.kind,
            turn.prompt,
            turn.baseline_effort,
            turn.care_effort,
            turn.baseline_goal,
            turn.care_goal,
            turn.baseline_talk,
            turn.care_talk,
            turn.words,
            turn.baseline_cost_ticks,
            turn.care_cost_ticks
        ));
    }
    out
}

fn sample_ms(system: &str, reply: &str) -> Result<f64, String> {
    let start = Instant::now();
    let req = ResponsesRequest {
        model: "scripted".into(),
        effort: None,
        input: vec![
            InputItem::Message {
                role: "system".into(),
                content: vec![ContentPart::InputText(system.into())],
            },
            InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("fixture".into())],
            },
        ],
        conversation_id: "care-first".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    };
    let out = Scripted(reply.to_string())
        .stream(&req, &CancelToken::new(), &mut |_| {})
        .map_err(|err| err.to_string())?;
    if out.text != reply {
        return Err("scripted reply did not match the fixture".into());
    }
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::brain_policy::builtin_care_first;
    use grokhub_core::BACKGROUND_EFFORT;

    #[test]
    fn care_first_default_on_needs_every_budget() {
        assert!(care_first_default_on(true, 1.25, 1.20, 1.40));
        assert!(!care_first_default_on(false, 1.0, 1.0, 1.0));
        assert!(!care_first_default_on(true, 1.26, 1.0, 1.0));
        assert!(!care_first_default_on(true, 1.0, 1.21, 1.0));
        assert!(!care_first_default_on(true, 1.0, 1.0, 1.41));
    }

    #[test]
    fn care_first_ab_stays_off_and_names_the_route_change() {
        let report = run_care_first_ab().expect("care-first ab");
        let expect = [
            (
                "emotional-rough",
                "emotional",
                "low",
                "medium",
                "care",
                "care",
                "natural, personality sets length",
                "natural, personality sets length",
                6,
            ),
            (
                "emotional-worry",
                "emotional",
                "low",
                "medium",
                "care",
                "care",
                "natural, personality sets length",
                "natural, personality sets length",
                13,
            ),
            (
                "open-afternoon",
                "open",
                "low",
                "low",
                "speed",
                "speed",
                "concise, under 150 words",
                "concise, under 150 words",
                13,
            ),
            (
                "open-hey",
                "open",
                "low",
                "medium",
                "care",
                "care",
                "natural, personality sets length",
                "natural, personality sets length",
                7,
            ),
            (
                "open-ramble",
                "open",
                "low",
                "medium",
                "care",
                "care",
                "natural, personality sets length",
                "natural, personality sets length",
                14,
            ),
            (
                "think-hard",
                "open",
                "xhigh",
                "medium",
                "care",
                "care",
                "natural, personality sets length",
                "natural, personality sets length",
                12,
            ),
        ];
        assert_eq!(report.turns.len(), expect.len());
        for (turn, row) in report.turns.iter().zip(expect) {
            assert_eq!(turn.id, row.0);
            assert_eq!(turn.kind, row.1);
            assert_eq!(turn.baseline_effort, row.2);
            assert_eq!(turn.care_effort, row.3);
            assert_eq!(turn.baseline_goal, row.4);
            assert_eq!(turn.care_goal, row.5);
            assert_eq!(turn.baseline_talk, row.6);
            assert_eq!(turn.care_talk, row.7);
            assert_eq!(turn.words, row.8);
        }
        assert_eq!(report.baseline_cost_ticks, 12_639);
        assert_eq!(report.care_cost_ticks, 12_328);
        assert!((report.cost_ratio - (12_328.0 / 12_639.0)).abs() < 1e-12);
        assert_eq!(report.baseline_words, 65);
        assert_eq!(report.care_words, 65);
        assert_eq!(report.words_ratio, 1.0);
        assert_eq!(report.baseline_sycophancy, 0.0);
        assert_eq!(report.care_sycophancy, 0.0);
        assert!(report.quality_judge.is_none());
        assert_eq!(report.classify_effort, "low");
        assert_eq!(report.care_classify_effort, "low");
        assert_eq!(report.classify_effort, BACKGROUND_EFFORT);
        assert!(!report.ships_on);
        assert!(!builtin_care_first());
        assert_eq!(report.ships_on, CARE_FIRST_SHIPS_ON);
        assert_eq!(CARE_FIRST_SHIPS_ON, builtin_care_first());
        assert!(!care_first_default_on(
            false,
            report.cost_ratio,
            report.p95_ratio,
            report.words_ratio
        ));
        assert!(report.baseline_p50_ms.is_finite() && report.baseline_p50_ms >= 0.0);
        assert!(report.care_p50_ms.is_finite() && report.care_p50_ms >= 0.0);
        assert!(report.baseline_p95_ms.is_finite() && report.baseline_p95_ms >= 0.0);
        assert!(report.care_p95_ms.is_finite() && report.care_p95_ms >= 0.0);
        assert!(report.why.contains("Skipped: live quality judge"));
        assert!(report.why.contains("Skipped: live model latency"));
        assert!(report.why.contains("12639"));
        assert!(report.why.contains("12328"));
        assert!(report.turns.iter().any(|turn| turn.kind == "emotional"));
        assert!(report.turns.iter().any(|turn| turn.kind == "open"));
    }
}
