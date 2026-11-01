//! Hand a card to the agent (now or queued), start a flow on it, and triage it:
//! priority, block with a reason, unblock, and its place in a column. The cabin
//! and the shell both call these, so a card reads the same in either app.
//!
//! A queued card waits until nothing else of yours is running: no open chat
//! turn, no other queued run, room under [`BG_TASK_MAX`], and no game or
//! anti-cheat running. The runner checks [`queue_wait`] before each start.

use serde::{Deserialize, Serialize};

use crate::bg_task::BG_TASK_MAX;
use crate::work_item::{
    clean_card, clip_line, StepStatus, WorkOrigin, WorkStep, CHECKPOINT_BLOCKER_MAX, STEPS_MAX,
    STEP_TEXT_MAX,
};
use crate::work_run::{checkpoint, next_step};
use crate::workboard::{BoardCard, BoardStatus, KanbanColumn};

/// What a card's flow is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    Playbook,
    Recipe,
    Automation,
}

impl FlowKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Playbook => "Playbook",
            Self::Recipe => "Recipe",
            Self::Automation => "Automation",
        }
    }
}

/// The playbook, recipe or automation started on a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardFlow {
    pub kind: FlowKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// Longest flow id or name kept on a card.
pub const FLOW_NAME_MAX: usize = 80;

impl CardFlow {
    pub fn new(kind: FlowKind, id: &str, name: &str) -> Self {
        Self {
            kind,
            id: clip_line(id, FLOW_NAME_MAX),
            name: clip_line(name, FLOW_NAME_MAX),
        }
    }

    /// "Recipe · Morning setup".
    pub fn line(&self) -> String {
        let name = if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        };
        format!("{} · {name}", self.kind.label())
    }
}

/// Priority values as stored, in the order the chip cycles: high, normal, low.
pub const PRIORITIES: [&str; 3] = ["high", "", "low"];

/// A priority as stored, or `None` for a word that is not one.
pub fn clean_priority(p: &str) -> Option<&'static str> {
    match p.trim().to_ascii_lowercase().as_str() {
        "high" | "urgent" | "p1" => Some("high"),
        "" | "normal" | "medium" | "med" | "p2" => Some(""),
        "low" | "p3" => Some("low"),
        _ => None,
    }
}

/// "High", "Normal" or "Low", as the chip reads.
pub fn priority_label(p: &str) -> &'static str {
    match clean_priority(p) {
        Some("high") => "High",
        Some("low") => "Low",
        _ => "Normal",
    }
}

fn find<'a>(cards: &'a mut [BoardCard], id: &str) -> Result<&'a mut BoardCard, String> {
    cards
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| "That card is gone.".to_string())
}

/// You handed it over or changed it: the card is yours and the sweep leaves it.
fn yours(c: &mut BoardCard) {
    c.origin = Some(WorkOrigin::User);
    c.touched = true;
    c.fresh = false;
}

fn may_hand_over(c: &BoardCard) -> Result<(), String> {
    match c.status {
        BoardStatus::Dismissed => Err("It is archived. Restore it first.".into()),
        BoardStatus::Done => Err("It is done. Move it back to Todo first.".into()),
        BoardStatus::Blocked => Err("It is blocked. Unblock it first.".into()),
        _ => Ok(()),
    }
}

/// Agent: do this → Now. The caller starts the run (the cabin's Work on it,
/// the shell's Work); this makes the card yours and takes it off the queue.
pub fn assign_now(cards: &mut [BoardCard], id: &str) -> Result<(), String> {
    let c = find(cards, id)?;
    may_hand_over(c)?;
    yours(c);
    c.queued_ms = 0;
    Ok(())
}

/// Agent: do this → Queue. True when it was not queued before.
pub fn queue_card(cards: &mut [BoardCard], id: &str, now: u64) -> Result<bool, String> {
    // Strictly after every card already queued, so two queued in the same
    // millisecond still go in the order you queued them.
    let last = cards.iter().map(|c| c.queued_ms).max().unwrap_or(0);
    let c = find(cards, id)?;
    may_hand_over(c)?;
    if c.status == BoardStatus::InProgress {
        return Err("It is already in Doing.".into());
    }
    yours(c);
    if c.queued_ms > 0 {
        return Ok(false);
    }
    c.queued_ms = now.max(last + 1);
    Ok(true)
}

/// Off the queue. True when it was queued.
pub fn unqueue_card(cards: &mut [BoardCard], id: &str) -> bool {
    match cards.iter_mut().find(|c| c.id == id && c.queued_ms > 0) {
        Some(c) => {
            c.queued_ms = 0;
            true
        }
        None => false,
    }
}

fn queueable(status: BoardStatus) -> bool {
    matches!(
        status.column(),
        Some(KanbanColumn::Todo | KanbanColumn::FollowUp)
    )
}

/// Waiting in the queue: queued, and still in Todo or Follow up.
pub fn is_queued(c: &BoardCard) -> bool {
    c.queued_ms > 0 && queueable(c.status)
}

/// The card the queue starts next: the one queued first.
pub fn next_queued(cards: &[BoardCard]) -> Option<&BoardCard> {
    cards
        .iter()
        .filter(|c| is_queued(c))
        .min_by_key(|c| (c.queued_ms, c.rank))
}

/// Where the card waits in the queue, from 1; `None` when it is not queued.
pub fn queue_place(cards: &[BoardCard], id: &str) -> Option<usize> {
    let mut queued: Vec<&BoardCard> = cards.iter().filter(|c| is_queued(c)).collect();
    queued.sort_by_key(|c| (c.queued_ms, c.rank));
    queued.iter().position(|c| c.id == id).map(|n| n + 1)
}

/// "Queued" for the first in line, then "Queued · 2nd" and so on.
pub fn queue_label(place: usize) -> String {
    match place {
        0 | 1 => "Queued".into(),
        2 => "Queued · 2nd".into(),
        3 => "Queued · 3rd".into(),
        n => format!("Queued · {n}th"),
    }
}

/// What the queue runner sees before it starts a card.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueGate {
    /// A chat turn of yours is running.
    pub foreground: bool,
    /// A card the queue started is still running.
    pub queued_live: bool,
    /// Background runs going now.
    pub bg_live: usize,
    /// Why the game guard says wait (a game or an anti-cheat is running).
    pub game: Option<String>,
    /// The agent can take a run (signed in, a model chosen).
    pub agent_ready: bool,
}

/// Why the queue waits, or `None` when the next card may start. The game guard
/// comes first: a queued run never starts while a protected game runs.
pub fn queue_wait(g: &QueueGate) -> Option<String> {
    if let Some(why) = g.game.as_ref() {
        return Some(why.clone());
    }
    if g.queued_live {
        return Some("A queued card is running. The next one waits for it.".into());
    }
    if g.foreground {
        return Some("Waits for the open chat to finish.".into());
    }
    if g.bg_live >= BG_TASK_MAX {
        return Some(format!(
            "Waits for one of the {BG_TASK_MAX} background runs to end."
        ));
    }
    if !g.agent_ready {
        return Some("Waits for the agent to be ready.".into());
    }
    None
}

/// The queue starts `id`: off the queue and to the end of Doing.
pub fn start_queued(cards: &mut [BoardCard], id: &str) -> bool {
    let rank = end_rank(cards, KanbanColumn::Doing);
    match cards.iter_mut().find(|c| c.id == id && is_queued(c)) {
        Some(c) => {
            c.queued_ms = 0;
            c.status = BoardStatus::InProgress;
            c.rank = rank;
            true
        }
        None => false,
    }
}

/// Set the priority chip. True when it changed.
pub fn set_priority(cards: &mut [BoardCard], id: &str, priority: &str) -> Result<bool, String> {
    let p = clean_priority(priority).ok_or_else(|| "Pick high, normal or low.".to_string())?;
    let c = find(cards, id)?;
    if c.priority == p {
        return Ok(false);
    }
    c.priority = p.to_string();
    c.touched = true;
    Ok(true)
}

/// Block with a reason, written into the checkpoint so a resume sees it. The
/// card moves to the end of Blocked and leaves the queue.
pub fn block_card(
    cards: &mut [BoardCard],
    id: &str,
    reason: &str,
    now: u64,
) -> Result<bool, String> {
    let reason = clip_line(reason, CHECKPOINT_BLOCKER_MAX);
    if reason.is_empty() {
        return Err("Say what it is waiting on.".into());
    }
    let rank = end_rank(cards, KanbanColumn::Blocked);
    let c = find(cards, id)?;
    if c.status == BoardStatus::Dismissed {
        return Err("It is archived. Restore it first.".into());
    }
    let same = c.status == BoardStatus::Blocked
        && c.checkpoint.as_ref().is_some_and(|cp| cp.blocker == reason);
    if same {
        return Ok(false);
    }
    if c.status != BoardStatus::Blocked {
        c.status = BoardStatus::Blocked;
        c.rank = rank;
    }
    c.queued_ms = 0;
    c.touched = true;
    let cp = checkpoint(c);
    cp.blocker = reason;
    cp.at_ms = now;
    clean_card(c);
    Ok(true)
}

/// Back to the end of Todo with the blocker cleared. True when it changed.
pub fn unblock_card(cards: &mut [BoardCard], id: &str, now: u64) -> bool {
    let rank = end_rank(cards, KanbanColumn::Todo);
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    let had_blocker = c
        .checkpoint
        .as_ref()
        .is_some_and(|cp| !cp.blocker.is_empty());
    if c.status != BoardStatus::Blocked && !had_blocker {
        return false;
    }
    if c.status == BoardStatus::Blocked {
        c.status = BoardStatus::Todo;
        c.rank = rank;
    }
    clear_blocker(c, now);
    c.touched = true;
    true
}

fn clear_blocker(c: &mut BoardCard, now: u64) {
    if let Some(cp) = c.checkpoint.as_mut().filter(|cp| !cp.blocker.is_empty()) {
        cp.blocker.clear();
        cp.at_ms = now;
    }
    clean_card(c);
}

/// A column's cards in their order: by rank, then as stored.
pub fn in_column(cards: &[BoardCard], column: KanbanColumn) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..cards.len())
        .filter(|&i| cards[i].status.column() == Some(column))
        .collect();
    idx.sort_by_key(|&i| cards[i].rank);
    idx
}

/// The rank that puts a card at the end of `column`.
pub fn end_rank(cards: &[BoardCard], column: KanbanColumn) -> i64 {
    in_column(cards, column)
        .last()
        .map(|&j| cards[j].rank.saturating_add(1))
        .unwrap_or(0)
}

/// Room left between ranks when a column has to be numbered again.
const RANK_GAP: i64 = 1024;

/// Put `id` in `column`, just above `before` (a card in that column), or at
/// its end. Only the moved card's rank changes, so a move never outvotes
/// another writer's edit of a neighbour in the merge; the column is numbered
/// again (with gaps) only when two neighbours leave no room between them. A
/// card that leaves Blocked loses its blocker; one that leaves Todo or Follow
/// up leaves the queue. True when anything moved.
pub fn place_card(
    cards: &mut [BoardCard],
    id: &str,
    column: KanbanColumn,
    before: Option<&str>,
    now: u64,
) -> bool {
    let Some(i) = cards.iter().position(|c| c.id == id) else {
        return false;
    };
    if cards[i].status == BoardStatus::Dismissed {
        return false;
    }
    let same_column = cards[i].status.column() == Some(column);
    let current = in_column(cards, column);
    let order: Vec<usize> = current.iter().copied().filter(|&j| j != i).collect();
    let at = before
        .filter(|b| *b != id)
        .and_then(|b| order.iter().position(|&j| cards[j].id == b))
        .unwrap_or(order.len());
    if same_column && current.iter().position(|&j| j == i) == Some(at) {
        return false;
    }
    if !same_column {
        let was_blocked = cards[i].status == BoardStatus::Blocked;
        cards[i].status = column.status();
        if was_blocked {
            clear_blocker(&mut cards[i], now);
        }
        if !queueable(cards[i].status) {
            cards[i].queued_ms = 0;
        }
    }
    let prev = at.checked_sub(1).map(|k| cards[order[k]].rank);
    let next = order.get(at).map(|&j| cards[j].rank);
    let rank = match (prev, next) {
        (None, None) => Some(0),
        (Some(p), None) => Some(p.saturating_add(1)),
        (None, Some(n)) => Some(n.saturating_sub(1)),
        (Some(p), Some(n)) if n.saturating_sub(p) >= 2 => Some(p + (n - p) / 2),
        _ => None,
    };
    match rank {
        Some(r) => cards[i].rank = r,
        None => {
            let mut order = order;
            order.insert(at, i);
            for (k, &j) in order.iter().enumerate() {
                cards[j].rank = (k as i64 + 1) * RANK_GAP;
            }
        }
    }
    cards[i].touched = true;
    true
}

fn mark_of(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Done => "[x]",
        StepStatus::Doing => "[>]",
        StepStatus::Skipped => "[-]",
        StepStatus::Failed => "[!]",
        StepStatus::Planned => "[ ]",
    }
}

/// The steps as you edit them: one a line, each with its mark.
pub fn steps_text(c: &BoardCard) -> String {
    c.steps
        .iter()
        .map(|s| format!("{} {}", mark_of(s.status), s.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One edited line: its mark, if it has one, and its text.
fn parse_step_line(line: &str) -> (Option<StepStatus>, String) {
    let t = line.trim().trim_start_matches(['-', '*']).trim_start();
    let marks = [
        ("[x]", StepStatus::Done),
        ("[X]", StepStatus::Done),
        ("[>]", StepStatus::Doing),
        ("[-]", StepStatus::Skipped),
        ("[!]", StepStatus::Failed),
        ("[ ]", StepStatus::Planned),
        ("[]", StepStatus::Planned),
    ];
    for (mark, status) in marks {
        if let Some(rest) = t.strip_prefix(mark) {
            return (Some(status), clip_line(rest, STEP_TEXT_MAX));
        }
    }
    (None, clip_line(t, STEP_TEXT_MAX))
}

/// Your edit of the steps, one a line. A line with no mark keeps the status
/// of the step with the same text, else it is planned. The plan becomes
/// yours (the agent may only move a step forward), and the checkpoint's next
/// step follows it, so the next resume works from your version. Clearing every
/// step hands planning back to the agent. True when anything changed.
pub fn edit_steps(c: &mut BoardCard, text: &str, now: u64) -> bool {
    let steps: Vec<WorkStep> = text
        .lines()
        .map(parse_step_line)
        .filter(|(_, t)| !t.is_empty())
        .take(STEPS_MAX)
        .map(|(mark, text)| {
            let kept = c
                .steps
                .iter()
                .find(|s| s.text.eq_ignore_ascii_case(&text))
                .map(|s| s.status);
            WorkStep {
                status: mark.or(kept).unwrap_or_default(),
                text,
            }
        })
        .collect();
    let user_steps = !steps.is_empty();
    if steps == c.steps && c.user_steps == user_steps {
        return false;
    }
    c.steps = steps;
    c.user_steps = user_steps;
    c.touched = true;
    let next = next_step(&c.steps);
    let cp = checkpoint(c);
    cp.next = next;
    cp.at_ms = now;
    clean_card(c);
    true
}

fn step_order(s: StepStatus) -> u8 {
    match s {
        StepStatus::Planned => 0,
        StepStatus::Doing => 1,
        StepStatus::Done => 2,
        StepStatus::Skipped | StepStatus::Failed => 3,
    }
}

/// May the agent set a step from `from` to `to`? On your plan it may only move
/// a step forward (planned to doing to done); your skips, fails and finished
/// steps stand.
pub fn agent_may_set(user_steps: bool, from: StepStatus, to: StepStatus) -> bool {
    if !user_steps || from == to {
        return true;
    }
    matches!(from, StepStatus::Planned | StepStatus::Doing)
        && matches!(to, StepStatus::Doing | StepStatus::Done)
        && step_order(to) > step_order(from)
}

/// The agent's plan (its to-dos) laid onto yours: your steps keep their text
/// and order, and a step the agent names the same moves forward where
/// [`agent_may_set`] allows. Steps the agent adds are not taken. True when a
/// status changed.
pub fn agent_onto_user_plan(c: &mut BoardCard, agent: &[WorkStep]) -> bool {
    let mut changed = false;
    for step in c.steps.iter_mut() {
        let theirs = agent
            .iter()
            .find(|a| a.text.trim().eq_ignore_ascii_case(step.text.trim()));
        if let Some(a) = theirs {
            if a.status != step.status && agent_may_set(true, step.status, a.status) {
                step.status = a.status;
                changed = true;
            }
        }
    }
    changed
}

/// Link a flow you started on the card. The card is yours.
pub fn link_flow(cards: &mut [BoardCard], id: &str, flow: CardFlow) -> bool {
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    yours(c);
    if c.flow.as_ref() == Some(&flow) {
        return false;
    }
    c.flow = Some(flow);
    true
}

/// Make it recurring: the schedules offered, as (label, words the schedule
/// parser reads).
pub const RECUR_CHOICES: [(&str, &str); 3] = [
    ("Every day at 9 AM", "every day at 9am"),
    ("Every weekday at 9 AM", "every weekday at 9am"),
    ("Every day at 6 PM", "every day at 6pm"),
];

/// The words that make the card's automation: the schedule, then the title.
pub fn recurring_seed(choice: usize, c: &BoardCard) -> Option<String> {
    let (_, when) = RECUR_CHOICES.get(choice)?;
    let title = clip_line(&c.title, 120);
    if title.is_empty() {
        return None;
    }
    Some(format!("{when} {title}"))
}

/// The new automation owns the card: its runs report here, as a Follow up does.
pub fn own_by_automation(
    cards: &mut [BoardCard],
    id: &str,
    automation_id: &str,
    name: &str,
) -> bool {
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    yours(c);
    c.queued_ms = 0;
    c.automation = Some(automation_id.to_string());
    c.kind = crate::work_item::WorkKind::Automation;
    c.flow = Some(CardFlow::new(FlowKind::Automation, automation_id, name));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_item::WorkKind;

    fn card(id: &str, status: BoardStatus, rank: i64) -> BoardCard {
        let mut c = BoardCard::new(id, "", "");
        c.id = id.into();
        c.status = status;
        c.rank = rank;
        c.origin = Some(WorkOrigin::Agent);
        c
    }

    fn steps(rows: &[(&str, StepStatus)]) -> Vec<WorkStep> {
        rows.iter()
            .map(|(t, s)| WorkStep {
                text: (*t).into(),
                status: *s,
            })
            .collect()
    }

    fn clear_gate() -> QueueGate {
        QueueGate {
            agent_ready: true,
            ..Default::default()
        }
    }

    #[test]
    fn queueing_makes_the_card_yours_and_the_oldest_goes_first() {
        let mut cards = vec![
            card("a", BoardStatus::Todo, 1),
            card("b", BoardStatus::Todo, 2),
            card("c", BoardStatus::Done, 1),
            card("d", BoardStatus::Blocked, 1),
        ];
        assert!(queue_card(&mut cards, "b", 100).unwrap());
        assert!(queue_card(&mut cards, "a", 200).unwrap());
        assert!(!queue_card(&mut cards, "a", 300).unwrap(), "already queued");
        assert!(queue_card(&mut cards, "c", 1).is_err(), "a done card waits");
        assert!(queue_card(&mut cards, "d", 1).is_err(), "unblock first");
        assert_eq!(cards[0].origin, Some(WorkOrigin::User));
        assert!(cards[0].touched && cards[1].touched);
        assert_eq!(next_queued(&cards).map(|c| c.id.as_str()), Some("b"));
        assert_eq!(queue_place(&cards, "a"), Some(2));
        assert_eq!(queue_label(2), "Queued · 2nd");
        assert!(start_queued(&mut cards, "b"));
        assert_eq!(cards[1].status, BoardStatus::InProgress);
        assert_eq!(next_queued(&cards).map(|c| c.id.as_str()), Some("a"));
        assert!(unqueue_card(&mut cards, "a"));
        assert!(next_queued(&cards).is_none());
    }

    #[test]
    fn the_queue_waits_for_a_game_the_chat_another_queued_run_and_room() {
        assert_eq!(queue_wait(&clear_gate()), None);
        let game = QueueGate {
            game: Some("A protected game is running.".into()),
            ..clear_gate()
        };
        assert_eq!(
            queue_wait(&game).as_deref(),
            Some("A protected game is running.")
        );
        for g in [
            QueueGate {
                foreground: true,
                ..clear_gate()
            },
            QueueGate {
                queued_live: true,
                ..clear_gate()
            },
            QueueGate {
                bg_live: BG_TASK_MAX,
                ..clear_gate()
            },
            QueueGate {
                agent_ready: false,
                ..clear_gate()
            },
        ] {
            assert!(queue_wait(&g).is_some(), "{g:?}");
        }
        assert_eq!(
            queue_wait(&QueueGate {
                bg_live: BG_TASK_MAX - 1,
                ..clear_gate()
            }),
            None
        );
    }

    #[test]
    fn a_block_reason_lands_in_the_checkpoint_and_unblock_clears_it() {
        let mut cards = vec![
            card("a", BoardStatus::Todo, 1),
            card("b", BoardStatus::Blocked, 4),
        ];
        cards[0].queued_ms = 5;
        assert!(block_card(&mut cards, "a", "  ", 10).is_err());
        assert!(block_card(&mut cards, "a", "Waiting on the API key", 10).unwrap());
        let a = &cards[0];
        assert_eq!(a.status, BoardStatus::Blocked);
        assert_eq!(a.rank, 5, "to the end of Blocked");
        assert_eq!(a.queued_ms, 0, "a blocked card leaves the queue");
        let cp = a.checkpoint.as_ref().unwrap();
        assert_eq!(cp.blocker, "Waiting on the API key");
        assert_eq!(cp.goal, "a", "the goal starts as the title");
        assert!(!block_card(&mut cards, "a", "Waiting on the API key", 11).unwrap());
        assert!(unblock_card(&mut cards, "a", 20));
        assert_eq!(cards[0].status, BoardStatus::Todo);
        assert_eq!(cards[0].checkpoint.as_ref().unwrap().blocker, "");
        assert!(!unblock_card(&mut cards, "a", 21));
    }

    #[test]
    fn a_drop_on_a_card_lands_above_it_and_moves_only_that_card() {
        let mut cards = vec![
            card("a", BoardStatus::Todo, 10),
            card("b", BoardStatus::Todo, 20),
            card("c", BoardStatus::Todo, 30),
            card("d", BoardStatus::Blocked, 1),
        ];
        cards[3].checkpoint = Some(crate::Checkpoint {
            goal: "d".into(),
            blocker: "Review".into(),
            ..Default::default()
        });
        assert!(place_card(
            &mut cards,
            "c",
            KanbanColumn::Todo,
            Some("a"),
            5
        ));
        let order: Vec<&str> = in_column(&cards, KanbanColumn::Todo)
            .iter()
            .map(|&i| cards[i].id.as_str())
            .collect();
        assert_eq!(order, ["c", "a", "b"]);
        assert!(
            !place_card(&mut cards, "c", KanbanColumn::Todo, Some("a"), 6),
            "no-op"
        );
        assert!(place_card(
            &mut cards,
            "d",
            KanbanColumn::Todo,
            Some("b"),
            7
        ));
        assert_eq!(cards[3].status, BoardStatus::Todo);
        assert_eq!(
            cards[3].checkpoint.as_ref().unwrap().blocker,
            "",
            "leaving Blocked unblocks"
        );
        let order: Vec<&str> = in_column(&cards, KanbanColumn::Todo)
            .iter()
            .map(|&i| cards[i].id.as_str())
            .collect();
        assert_eq!(order, ["c", "a", "d", "b"]);
        assert_eq!(cards[1].rank, 20, "a neighbour keeps its rank");
        assert!(place_card(&mut cards, "a", KanbanColumn::Done, None, 8));
        assert_eq!(cards[0].status, BoardStatus::Done);
        assert!(cards[0].touched);

        // No room between two neighbours: the column is numbered again, with gaps.
        let mut tight = vec![
            card("x", BoardStatus::Todo, 1),
            card("y", BoardStatus::Todo, 2),
            card("z", BoardStatus::Todo, 3),
        ];
        assert!(place_card(
            &mut tight,
            "z",
            KanbanColumn::Todo,
            Some("y"),
            9
        ));
        let ranks: Vec<(&str, i64)> = in_column(&tight, KanbanColumn::Todo)
            .iter()
            .map(|&i| (tight[i].id.as_str(), tight[i].rank))
            .collect();
        assert_eq!(ranks, [("x", 1024), ("z", 2048), ("y", 3072)]);
    }

    #[test]
    fn your_step_edit_wins_over_the_agents_plan() {
        let mut c = card("a", BoardStatus::InProgress, 1);
        c.steps = steps(&[
            ("Read the tray code", StepStatus::Done),
            ("Patch the DPI scaling", StepStatus::Doing),
            ("Re-run the tests", StepStatus::Planned),
        ]);
        let text = steps_text(&c);
        assert_eq!(
            text,
            "[x] Read the tray code\n[>] Patch the DPI scaling\n[ ] Re-run the tests"
        );
        let edited = "[x] Read the tray code\n[-] Patch the DPI scaling\nTest on a 4K screen\nRe-run the tests";
        assert!(edit_steps(&mut c, edited, 50));
        assert!(c.user_steps && c.touched);
        assert_eq!(c.steps.len(), 4);
        assert_eq!(c.steps[1].status, StepStatus::Skipped);
        assert_eq!(
            c.steps[3].status,
            StepStatus::Planned,
            "same text keeps its status"
        );
        assert_eq!(c.checkpoint.as_ref().unwrap().next, "Test on a 4K screen");
        assert!(
            !edit_steps(&mut c, edited, 60),
            "the same edit changes nothing"
        );

        // The agent's own plan, with the old step and one of its own.
        let agent = steps(&[
            ("Read the tray code", StepStatus::Done),
            ("Patch the DPI scaling", StepStatus::Doing),
            ("Test on a 4K screen", StepStatus::Done),
            ("Ship it", StepStatus::Planned),
        ]);
        assert!(agent_onto_user_plan(&mut c, &agent));
        assert_eq!(c.steps[1].status, StepStatus::Skipped, "your skip stands");
        assert_eq!(
            c.steps[2].status,
            StepStatus::Done,
            "forward progress is taken"
        );
        assert_eq!(c.steps.len(), 4, "its extra step is not added");
        assert!(!agent_may_set(true, StepStatus::Done, StepStatus::Planned));
        assert!(!agent_may_set(
            true,
            StepStatus::Planned,
            StepStatus::Skipped
        ));
        assert!(agent_may_set(false, StepStatus::Done, StepStatus::Planned));

        assert!(edit_steps(&mut c, " \n", 70));
        assert!(!c.user_steps, "clearing the plan hands it back");
    }

    #[test]
    fn priority_flow_and_recurring_shapes() {
        let mut cards = vec![card("a", BoardStatus::Todo, 1)];
        assert!(set_priority(&mut cards, "a", "High").unwrap());
        assert_eq!(cards[0].priority, "high");
        assert!(!set_priority(&mut cards, "a", "urgent").unwrap());
        assert!(set_priority(&mut cards, "a", "bogus").is_err());
        assert_eq!(priority_label("low"), "Low");
        assert_eq!(priority_label(""), "Normal");

        let flow = CardFlow::new(FlowKind::Recipe, "morning", "Morning setup");
        assert!(link_flow(&mut cards, "a", flow.clone()));
        assert!(!link_flow(&mut cards, "a", flow));
        assert_eq!(
            cards[0].flow.as_ref().unwrap().line(),
            "Recipe · Morning setup"
        );

        cards[0].title = "Summarize the build".into();
        for (i, (_, when)) in RECUR_CHOICES.iter().enumerate() {
            let seed = recurring_seed(i, &cards[0]).unwrap();
            assert_eq!(seed, format!("{when} Summarize the build"));
            let a = crate::parse_nl_automation(&seed).expect("the schedule parser reads it");
            assert!(!a.name.trim().is_empty());
        }
        let evening = crate::parse_nl_automation(&recurring_seed(2, &cards[0]).unwrap()).unwrap();
        assert_eq!(
            (evening.schedule.as_str(), evening.time.as_str()),
            ("daily", "18:00")
        );
        assert_eq!(evening.name, "Summarize the build");
        assert!(recurring_seed(9, &cards[0]).is_none());
        assert!(own_by_automation(
            &mut cards,
            "a",
            "auto-1",
            "Summarize the build"
        ));
        assert_eq!(cards[0].automation.as_deref(), Some("auto-1"));
        assert_eq!(cards[0].kind, WorkKind::Automation);
        assert_eq!(cards[0].flow.as_ref().unwrap().kind, FlowKind::Automation);
    }
}
