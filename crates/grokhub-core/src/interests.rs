//! The person's interest profile: topics Pulse looks for, with a weight each.
//! Seeded once from USER.md, MEMORY.md, recent chats and liked cards; edited in
//! Settings → Pulse; nudged by what they open, save and skip.
//!
//! The interest graph records cross-chat evidence and domain-general edges
//! (related / maker / category / adjacent) so one mention is not an identity.

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "interests.json";
/// Longest topic kept, in chars.
pub const TOPIC_CHARS: usize = 40;
/// Most topics a profile holds. Adding past this drops the weakest unpinned one.
pub const TOPICS_MAX: usize = 40;
/// Most topics one seeding pass adds.
pub const SEED_MAX: usize = 12;
/// Topics named in the lookup prompt, strongest first.
pub const STEER_MAX: usize = 6;
pub const WEIGHT_MIN: u8 = 5;
pub const WEIGHT_MAX: u8 = 100;

/// Distinct chats that must name a topic before a Chat-sourced Interest is promoted.
pub const EVIDENCE_CHATS_MIN: usize = 2;
/// Distinct co-mention chats before an edge may be probed.
pub const EDGE_EVIDENCE_MIN: usize = 2;
/// Neighbor probe searches per source check.
pub const PROBE_MAX: usize = 2;
/// Starting weight for a new related edge.
pub const EDGE_WEIGHT_START: i16 = 30;
/// Minimum open-edge weight before Pulse probes the neighbor (at start weight once evidence is met).
pub const EDGE_WEIGHT_PROBE: i16 = EDGE_WEIGHT_START;
pub const EDGE_WEIGHT_MAX: i16 = 100;
/// Chat ids kept per evidence / edge row.
pub const EVIDENCE_CHAT_IDS_MAX: usize = 8;
/// Cap on evidence rows in the local file.
pub const EVIDENCE_ROWS_MAX: usize = 80;
/// Cap on edges in the local file.
pub const EDGES_MAX: usize = 120;
/// Recent chats observe() reads outside the one-shot seed.
pub const OBSERVE_CHATS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterestSource {
    /// From USER.md or MEMORY.md.
    Memory,
    /// From a recent chat title.
    Chats,
    /// From a card they liked or saved.
    Saved,
    /// Typed in Settings.
    Explicit,
}

impl InterestSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "From memory",
            Self::Chats => "From chats",
            Self::Saved => "From saved",
            Self::Explicit => "Added by you",
        }
    }

    /// Where a new topic starts.
    pub fn start_weight(self) -> u8 {
        match self {
            Self::Explicit => 70,
            Self::Memory => 60,
            Self::Saved => 55,
            Self::Chats => 40,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interest {
    pub topic: String,
    pub weight: u8,
    pub source: InterestSource,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub muted: bool,
    /// When it was added, or last named by a card the person opened, saved or skipped.
    #[serde(default)]
    pub last_seen_ms: u64,
}

/// What the person did with a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterestSignal {
    Opened,
    Saved,
    Accepted,
    Dismissed,
    NotInterested,
    /// More like this on a Pulse post.
    MoreLikeThis,
}

impl InterestSignal {
    pub fn delta(self) -> i16 {
        match self {
            Self::Opened => 4,
            Self::Saved => 10,
            Self::Accepted => 8,
            Self::Dismissed => -6,
            Self::NotInterested => -15,
            Self::MoreLikeThis => 12,
        }
    }

    fn edge_delta(self) -> i16 {
        match self {
            Self::Opened => 4,
            Self::Saved | Self::MoreLikeThis => 12,
            Self::Accepted => 8,
            Self::Dismissed => -6,
            Self::NotInterested => 0,
        }
    }
}

/// Domain-general link between two topics. Gaming genre/developer is one instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Topics travel together across chats.
    Related,
    /// Creator / maker / author / org (generic; not a product API).
    Maker,
    /// Category / genre / class (generic label).
    Category,
    /// Branch after a closed link so probes go elsewhere.
    Adjacent,
}

/// Mentions of one topic across distinct chats. Not yet a steerable Interest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicEvidence {
    pub topic: String,
    #[serde(default)]
    pub chat_ids: Vec<String>,
    #[serde(default)]
    pub mention_count: u32,
    #[serde(default)]
    pub last_seen_ms: u64,
}

/// A weighted link in the interest connection map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterestEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub weight: i16,
    #[serde(default)]
    pub chat_ids: Vec<String>,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub last_seen_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterestGraph {
    #[serde(default)]
    pub evidence: Vec<TopicEvidence>,
    #[serde(default)]
    pub edges: Vec<InterestEdge>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterestProfile {
    #[serde(default)]
    pub topics: Vec<Interest>,
    /// Seeding ran once; later passes leave the list to the person.
    #[serde(default)]
    pub seeded: bool,
    /// Lowercased topics the person removed. Seeding never brings them back.
    #[serde(default)]
    pub removed: Vec<String>,
    /// Cross-chat evidence and connection edges.
    #[serde(default)]
    pub graph: InterestGraph,
}

/// Trimmed, single-spaced, no bullet or trailing period, at most TOPIC_CHARS.
pub fn clean_topic(raw: &str) -> String {
    let words: Vec<&str> = raw
        .trim()
        .trim_start_matches(['-', '*', '•'])
        .trim_end_matches(['.', '!', ';', ','])
        .split_whitespace()
        .collect();
    let joined = words.join(" ");
    let mut out: String = joined.chars().take(TOPIC_CHARS).collect();
    if out.chars().count() < joined.chars().count() {
        if let Some(cut) = out.rfind(' ') {
            out.truncate(cut);
        }
    }
    out.trim().to_string()
}

fn key(topic: &str) -> String {
    clean_topic(topic).to_lowercase()
}

impl InterestProfile {
    pub fn find(&self, topic: &str) -> Option<&Interest> {
        let k = key(topic);
        self.topics.iter().find(|t| t.topic.to_lowercase() == k)
    }

    fn find_mut(&mut self, topic: &str) -> Option<&mut Interest> {
        let k = key(topic);
        self.topics.iter_mut().find(|t| t.topic.to_lowercase() == k)
    }

    /// Add a topic. The person's own add always lands and clears an earlier
    /// removal; anything else skips topics they removed. False when nothing changed.
    pub fn add(&mut self, topic: &str, source: InterestSource, now_ms: u64) -> bool {
        let topic = clean_topic(topic);
        if topic.is_empty() {
            return false;
        }
        let k = topic.to_lowercase();
        if source == InterestSource::Explicit {
            self.removed.retain(|r| *r != k);
        } else if self.removed.contains(&k) {
            return false;
        }
        if let Some(have) = self.find_mut(&topic) {
            if source == InterestSource::Explicit && have.source != InterestSource::Explicit {
                have.source = InterestSource::Explicit;
                have.weight = have.weight.max(InterestSource::Explicit.start_weight());
                return true;
            }
            return false;
        }
        if self.topics.len() >= TOPICS_MAX && !self.drop_weakest() {
            return false;
        }
        self.topics.push(Interest {
            topic,
            weight: source.start_weight(),
            source,
            pinned: false,
            muted: false,
            last_seen_ms: now_ms,
        });
        true
    }

    fn drop_weakest(&mut self) -> bool {
        let weakest = self
            .topics
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.pinned && t.source != InterestSource::Explicit)
            .min_by_key(|(_, t)| (t.weight, t.last_seen_ms))
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                self.topics.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, topic: &str) -> bool {
        let k = key(topic);
        let before = self.topics.len();
        self.topics.retain(|t| t.topic.to_lowercase() != k);
        if self.topics.len() == before {
            return false;
        }
        if !self.removed.contains(&k) {
            self.removed.push(k);
        }
        true
    }

    pub fn set_pinned(&mut self, topic: &str, pinned: bool) -> bool {
        match self.find_mut(topic) {
            Some(t) if t.pinned != pinned => {
                t.pinned = pinned;
                if pinned {
                    t.muted = false;
                }
                true
            }
            _ => false,
        }
    }

    pub fn set_muted(&mut self, topic: &str, muted: bool) -> bool {
        match self.find_mut(topic) {
            Some(t) if t.muted != muted => {
                t.muted = muted;
                if muted {
                    t.pinned = false;
                }
                true
            }
            _ => false,
        }
    }

    /// Nudge every topic the card's text names. A pinned topic never drops.
    /// Returns the topics that moved.
    pub fn feedback(&mut self, text: &str, signal: InterestSignal, now_ms: u64) -> Vec<String> {
        self.feedback_anchored(text, None, signal, now_ms)
    }

    /// Like [`feedback`], and update graph edges using an optional anchor interest
    /// (the card's `because` topic) so dislike can close that link.
    pub fn feedback_anchored(
        &mut self,
        text: &str,
        anchor: Option<&str>,
        signal: InterestSignal,
        now_ms: u64,
    ) -> Vec<String> {
        let delta = signal.delta();
        let mut moved = Vec::new();
        for t in &mut self.topics {
            if !names_topic(text, &t.topic) {
                continue;
            }
            t.last_seen_ms = now_ms;
            if delta < 0 && t.pinned {
                continue;
            }
            let next =
                (i16::from(t.weight) + delta).clamp(i16::from(WEIGHT_MIN), i16::from(WEIGHT_MAX));
            let next = u8::try_from(next).unwrap_or(WEIGHT_MIN);
            if next != t.weight {
                t.weight = next;
                moved.push(t.topic.clone());
            }
        }
        self.edge_feedback(text, anchor, signal, now_ms);
        moved
    }

    /// Unmuted topics, pinned first, then by weight.
    pub fn ranked(&self) -> Vec<&Interest> {
        let mut out: Vec<&Interest> = self.topics.iter().filter(|t| !t.muted).collect();
        out.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.weight.cmp(&a.weight))
                .then(a.topic.cmp(&b.topic))
        });
        out
    }

    /// Topics with enough evidence (or Explicit / Memory / Saved / pinned).
    pub fn ready_ranked(&self) -> Vec<&Interest> {
        self.ranked()
            .into_iter()
            .filter(|t| self.topic_is_ready(t))
            .collect()
    }

    /// True when this Interest may steer Pulse / digests.
    pub fn topic_is_ready(&self, t: &Interest) -> bool {
        if t.muted {
            return false;
        }
        if t.pinned
            || matches!(
                t.source,
                InterestSource::Explicit | InterestSource::Memory | InterestSource::Saved
            )
        {
            return true;
        }
        if t.weight > InterestSource::Chats.start_weight() {
            return true;
        }
        self.graph
            .evidence
            .iter()
            .find(|e| e.topic.eq_ignore_ascii_case(&t.topic))
            .is_some_and(|e| e.chat_ids.len() >= EVIDENCE_CHATS_MIN)
    }

    /// Topics strong enough to search (HN / Reddit / DuckDuckGo Lite). Ready
    /// interests only, then skips cabin chrome, agent lane names, stopwords and
    /// weak chat unigrams. Empty when nothing qualifies — no fallback query.
    pub fn search_topics(&self) -> Vec<String> {
        self.ready_ranked()
            .into_iter()
            .filter(|t| searchable_interest(t))
            .map(|t| t.topic.clone())
            .collect()
    }

    /// The prompt section the lookup follows. Empty when there is nothing to say.
    pub fn prompt_section(&self) -> String {
        let top = self
            .search_topics()
            .into_iter()
            .take(STEER_MAX)
            .collect::<Vec<_>>();
        let muted: Vec<&str> = self
            .topics
            .iter()
            .filter(|t| t.muted)
            .map(|t| t.topic.as_str())
            .collect();
        let mut out = String::new();
        if !top.is_empty() {
            out.push_str(&format!(
                "Their interests, strongest first: {}.",
                top.join(", ")
            ));
        }
        if !muted.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("Never about: {}.", muted.join(", ")));
        }
        out
    }

    /// Record mentions from one chat. Does not add steerable Interests by itself.
    pub fn observe_chat(&mut self, chat_id: &str, title: &str, now_ms: u64) {
        let chat_id = chat_id.trim();
        if chat_id.is_empty() {
            return;
        }
        let candidates = self.candidates_from_title(title);
        if candidates.is_empty() {
            return;
        }
        for topic in &candidates {
            self.graph.note_mention(topic, chat_id, now_ms);
        }
        for i in 0..candidates.len() {
            for j in (i + 1)..candidates.len() {
                self.graph.note_cooccur(
                    &candidates[i],
                    &candidates[j],
                    chat_id,
                    EdgeKind::Related,
                    now_ms,
                );
            }
        }
        self.graph.trim();
    }

    /// Known topics the title names, else a short title. Preferring known topics
    /// keeps edges on real entities instead of whole-title phrases.
    fn candidates_from_title(&self, title: &str) -> Vec<String> {
        let mut out = Vec::new();
        let push = |out: &mut Vec<String>, topic: String| {
            if self.removed.iter().any(|r| r == &key(&topic)) || !topic_searchable(&topic) {
                return;
            }
            if !out.iter().any(|t| t.eq_ignore_ascii_case(&topic)) {
                out.push(topic);
            }
        };
        for t in &self.topics {
            if names_topic(title, &t.topic) {
                push(&mut out, t.topic.clone());
            }
        }
        for e in &self.graph.evidence {
            if names_topic(title, &e.topic) {
                push(&mut out, e.topic.clone());
            }
        }
        if out.is_empty() {
            if let Some(topic) = short_title(title) {
                push(&mut out, topic);
            }
        }
        out
    }

    /// Observe many chats (id, title), newest first is fine; order does not matter.
    pub fn observe_chats(&mut self, chats: &[(String, String)], now_ms: u64) {
        for (id, title) in chats {
            self.observe_chat(id, title, now_ms);
        }
    }

    /// Promote Chat Interests from evidence that crossed the threshold.
    /// Returns how many topics were added.
    pub fn promote_from_evidence(&mut self, now_ms: u64) -> usize {
        let mut added = 0;
        let ready: Vec<String> = self
            .graph
            .evidence
            .iter()
            .filter(|e| e.chat_ids.len() >= EVIDENCE_CHATS_MIN && topic_searchable(&e.topic))
            .map(|e| e.topic.clone())
            .collect();
        for topic in ready {
            if self.add(&topic, InterestSource::Chats, now_ms) {
                added += 1;
            }
        }
        added
    }

    /// Neighbor topics Pulse should probe this check. `topic` on each Related is the
    /// anchor Interest (for `because`); `query` is the neighbor string.
    pub fn probe_queries(&self, max: usize) -> Vec<crate::pulse_sources::Related> {
        if max == 0 {
            return Vec::new();
        }
        let anchors: Vec<String> = self
            .ready_ranked()
            .into_iter()
            .map(|t| t.topic.clone())
            .collect();
        let mut out = Vec::new();
        let mut seen_q = std::collections::BTreeSet::new();
        for anchor in &anchors {
            if out.len() >= max {
                break;
            }
            let mut edges: Vec<&InterestEdge> = self
                .graph
                .edges
                .iter()
                .filter(|e| !e.closed && e.from.eq_ignore_ascii_case(anchor))
                .filter(|e| e.chat_ids.len() >= EDGE_EVIDENCE_MIN && e.weight >= EDGE_WEIGHT_PROBE)
                .filter(|e| {
                    topic_searchable(&e.to)
                        && !self.removed.iter().any(|r| r == &key(&e.to))
                        && !self
                            .topics
                            .iter()
                            .any(|t| t.muted && t.topic.eq_ignore_ascii_case(&e.to))
                })
                .collect();
            edges.sort_by(|a, b| {
                b.weight
                    .cmp(&a.weight)
                    .then(b.last_seen_ms.cmp(&a.last_seen_ms))
                    .then(a.to.cmp(&b.to))
            });
            for e in edges {
                if out.len() >= max {
                    break;
                }
                let q = e.to.clone();
                if anchors.iter().any(|a| a.eq_ignore_ascii_case(&q)) {
                    continue;
                }
                if !seen_q.insert(q.to_lowercase()) {
                    continue;
                }
                out.push(crate::pulse_sources::Related {
                    query: q,
                    topic: Some(anchor.clone()),
                });
            }
        }
        out
    }

    /// Update connection edges from a card action without changing topic weights.
    /// Call this when weights already moved (e.g. via `pulse_note` + title match).
    pub fn edge_feedback(
        &mut self,
        text: &str,
        anchor: Option<&str>,
        signal: InterestSignal,
        now_ms: u64,
    ) {
        let named: Vec<String> = self
            .topics
            .iter()
            .filter(|t| names_topic(text, &t.topic))
            .map(|t| t.topic.clone())
            .collect();
        let mut ends = named;
        if let Some(a) = anchor.map(clean_topic).filter(|a| !a.is_empty()) {
            if !ends.iter().any(|t| t.eq_ignore_ascii_case(&a)) {
                ends.push(a);
            }
        }
        if ends.len() < 2 && signal != InterestSignal::NotInterested {
            // Strengthen edges between any two ready/evidence topics named in text.
            let evidenced: Vec<String> = self
                .graph
                .evidence
                .iter()
                .filter(|e| names_topic(text, &e.topic))
                .map(|e| e.topic.clone())
                .collect();
            for t in evidenced {
                if !ends.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                    ends.push(t);
                }
            }
        }
        match signal {
            InterestSignal::NotInterested => {
                let anchor_key = anchor
                    .map(clean_topic)
                    .filter(|a| !a.is_empty())
                    .or_else(|| ends.first().cloned());
                let Some(from) = anchor_key else {
                    return;
                };
                let mut targets: Vec<String> = ends
                    .iter()
                    .filter(|t| !t.eq_ignore_ascii_case(&from))
                    .cloned()
                    .collect();
                // Close links to neighbors named in the card even when they are not Interests yet.
                for e in &self.graph.edges {
                    if e.closed || !e.from.eq_ignore_ascii_case(&from) {
                        continue;
                    }
                    if names_topic(text, &e.to)
                        && !targets.iter().any(|t| t.eq_ignore_ascii_case(&e.to))
                    {
                        targets.push(e.to.clone());
                    }
                }
                let mut closed_from = false;
                for to in &targets {
                    if self.graph.close_edge(&from, to, now_ms) {
                        closed_from = true;
                    }
                }
                if closed_from || targets.is_empty() {
                    self.graph.branch_adjacent(&from, &targets, now_ms);
                }
            }
            _ => {
                let delta = signal.edge_delta();
                if delta == 0 || ends.len() < 2 {
                    return;
                }
                for i in 0..ends.len() {
                    for j in (i + 1)..ends.len() {
                        self.graph.nudge_pair(&ends[i], &ends[j], delta, now_ms);
                    }
                }
            }
        }
        self.graph.trim();
    }

    /// One seeding pass from what the app already knows. Runs once.
    /// Returns how many topics it added.
    pub fn seed(&mut self, from: &SeedFrom<'_>, now_ms: u64) -> usize {
        if self.seeded {
            return 0;
        }
        self.seeded = true;
        let mut added = 0;
        let mut offer = |p: &mut Self, topic: &str, source: InterestSource| {
            if !topic_searchable(topic) {
                return;
            }
            if added < SEED_MAX && p.add(topic, source, now_ms) {
                added += 1;
            }
        };
        for raw in [from.user_md, from.memory_md] {
            for topic in topics_in_memory(raw) {
                offer(self, &topic, InterestSource::Memory);
            }
        }
        for title in from.saved_titles {
            if let Some(topic) = short_title(title) {
                offer(self, &topic, InterestSource::Saved);
            }
        }
        // Chats: observe + promote (need two distinct chats), never one-shot identity.
        self.observe_chats(from.chats, now_ms);
        added += self.promote_from_evidence(now_ms);
        added
    }
}

impl InterestGraph {
    fn note_mention(&mut self, topic: &str, chat_id: &str, now_ms: u64) {
        let topic = clean_topic(topic);
        if topic.is_empty() || !topic_searchable(&topic) {
            return;
        }
        if let Some(row) = self
            .evidence
            .iter_mut()
            .find(|e| e.topic.eq_ignore_ascii_case(&topic))
        {
            row.mention_count = row.mention_count.saturating_add(1);
            row.last_seen_ms = now_ms;
            push_chat_id(&mut row.chat_ids, chat_id);
            return;
        }
        self.evidence.push(TopicEvidence {
            topic,
            chat_ids: vec![chat_id.to_string()],
            mention_count: 1,
            last_seen_ms: now_ms,
        });
    }

    fn note_cooccur(&mut self, a: &str, b: &str, chat_id: &str, kind: EdgeKind, now_ms: u64) {
        let a = clean_topic(a);
        let b = clean_topic(b);
        if a.is_empty() || b.is_empty() || a.eq_ignore_ascii_case(&b) {
            return;
        }
        if !topic_searchable(&a) || !topic_searchable(&b) {
            return;
        }
        // Directed: each endpoint can probe the other.
        self.upsert_edge(&a, &b, kind, chat_id, now_ms);
        self.upsert_edge(&b, &a, kind, chat_id, now_ms);
    }

    fn upsert_edge(&mut self, from: &str, to: &str, kind: EdgeKind, chat_id: &str, now_ms: u64) {
        if let Some(e) = self.edges.iter_mut().find(|e| {
            e.from.eq_ignore_ascii_case(from) && e.to.eq_ignore_ascii_case(to) && e.kind == kind
        }) {
            if e.closed {
                return;
            }
            e.last_seen_ms = now_ms;
            e.weight = (e.weight + 4).clamp(EDGE_WEIGHT_START, EDGE_WEIGHT_MAX);
            push_chat_id(&mut e.chat_ids, chat_id);
            return;
        }
        self.edges.push(InterestEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            weight: EDGE_WEIGHT_START,
            chat_ids: vec![chat_id.to_string()],
            closed: false,
            last_seen_ms: now_ms,
        });
    }

    fn nudge_pair(&mut self, a: &str, b: &str, delta: i16, now_ms: u64) {
        for (from, to) in [(a, b), (b, a)] {
            if let Some(e) = self.edges.iter_mut().find(|e| {
                e.from.eq_ignore_ascii_case(from) && e.to.eq_ignore_ascii_case(to) && !e.closed
            }) {
                e.weight = (e.weight + delta).clamp(1, EDGE_WEIGHT_MAX);
                e.last_seen_ms = now_ms;
            } else if delta > 0 {
                self.edges.push(InterestEdge {
                    from: clean_topic(from),
                    to: clean_topic(to),
                    kind: EdgeKind::Related,
                    weight: EDGE_WEIGHT_START.saturating_add(delta).min(EDGE_WEIGHT_MAX),
                    chat_ids: Vec::new(),
                    closed: false,
                    last_seen_ms: now_ms,
                });
            }
        }
    }

    fn close_edge(&mut self, from: &str, to: &str, now_ms: u64) -> bool {
        let mut closed = false;
        for e in &mut self.edges {
            if e.closed {
                continue;
            }
            let match_dir = (e.from.eq_ignore_ascii_case(from) && e.to.eq_ignore_ascii_case(to))
                || (e.from.eq_ignore_ascii_case(to) && e.to.eq_ignore_ascii_case(from));
            if match_dir {
                e.closed = true;
                e.last_seen_ms = now_ms;
                closed = true;
            }
        }
        closed
    }

    /// After closing links from `from`, open or bump adjacent edges to other neighbors.
    fn branch_adjacent(&mut self, from: &str, skip: &[String], now_ms: u64) {
        let neighbors: Vec<String> = self
            .edges
            .iter()
            .filter(|e| e.from.eq_ignore_ascii_case(from) && !e.closed)
            .map(|e| e.to.clone())
            .filter(|to| !skip.iter().any(|s| s.eq_ignore_ascii_case(to)))
            .collect();
        for to in neighbors {
            if let Some(e) = self.edges.iter_mut().find(|e| {
                e.from.eq_ignore_ascii_case(from)
                    && e.to.eq_ignore_ascii_case(&to)
                    && e.kind == EdgeKind::Adjacent
            }) {
                e.closed = false;
                e.weight = (e.weight + 8).clamp(EDGE_WEIGHT_PROBE, EDGE_WEIGHT_MAX);
                e.last_seen_ms = now_ms;
                if e.chat_ids.len() < EDGE_EVIDENCE_MIN {
                    // Synthetic evidence so the branch can probe once.
                    while e.chat_ids.len() < EDGE_EVIDENCE_MIN {
                        e.chat_ids.push(format!("branch-{}", e.chat_ids.len()));
                    }
                }
            } else {
                self.edges.push(InterestEdge {
                    from: clean_topic(from),
                    to: clean_topic(&to),
                    kind: EdgeKind::Adjacent,
                    weight: EDGE_WEIGHT_PROBE,
                    chat_ids: vec!["branch-0".into(), "branch-1".into()],
                    closed: false,
                    last_seen_ms: now_ms,
                });
            }
        }
    }

    fn trim(&mut self) {
        if self.evidence.len() > EVIDENCE_ROWS_MAX {
            self.evidence
                .sort_by_key(|a| std::cmp::Reverse(a.last_seen_ms));
            self.evidence.truncate(EVIDENCE_ROWS_MAX);
        }
        if self.edges.len() > EDGES_MAX {
            self.edges.sort_by(|a, b| {
                a.closed
                    .cmp(&b.closed)
                    .then(b.weight.cmp(&a.weight))
                    .then(b.last_seen_ms.cmp(&a.last_seen_ms))
            });
            self.edges.truncate(EDGES_MAX);
        }
    }
}

fn push_chat_id(ids: &mut Vec<String>, chat_id: &str) {
    if ids.iter().any(|c| c == chat_id) {
        return;
    }
    ids.push(chat_id.to_string());
    if ids.len() > EVIDENCE_CHAT_IDS_MAX {
        let drop = ids.len() - EVIDENCE_CHAT_IDS_MAX;
        ids.drain(..drop);
    }
}

/// True when this interest may become a Pulse source query.
fn searchable_interest(t: &Interest) -> bool {
    if !topic_searchable(&t.topic) {
        return false;
    }
    // Prefer sealed / strong sources. Chat titles only when multi-word.
    if t.pinned
        || matches!(
            t.source,
            InterestSource::Explicit | InterestSource::Memory | InterestSource::Saved
        )
    {
        return true;
    }
    t.topic.split_whitespace().count() >= 2
}

/// A topic string safe to put in HN / Reddit / Lite `q=`. Blocks cabin chrome,
/// agent class names (`background:triage`), and weak unigrams.
pub fn topic_searchable(topic: &str) -> bool {
    let topic = clean_topic(topic);
    if topic.is_empty() {
        return false;
    }
    let lower = topic.to_lowercase();
    // Agent lanes and class ids: "background:triage", "repair:diagnose".
    if lower.contains(':') {
        return false;
    }
    if CABIN_TOPIC_BLOCK.contains(&lower.as_str()) {
        return false;
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.is_empty() {
        return false;
    }
    if words.len() == 1 && WEAK_UNIGRAMS.contains(&words[0]) {
        return false;
    }
    // "the background", "new chat" style leftovers.
    if words.iter().all(|w| WEAK_UNIGRAMS.contains(w)) {
        return false;
    }
    true
}

/// What seeding reads. Chats are (id, title), newest first is fine.
pub struct SeedFrom<'a> {
    pub user_md: &'a str,
    pub memory_md: &'a str,
    pub chats: &'a [(String, String)],
    pub saved_titles: &'a [String],
}

/// Most chat titles seeding reads.
pub const SEED_CHATS: usize = 5;

/// Whole-word, case-insensitive: "rust" names "Rust 1.99 is out", not "trust".
pub fn names_topic(text: &str, topic: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let hay = words(text);
    let needle = words(topic);
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

const MEMORY_MARKERS: &[&str] = &[
    "interests:",
    "interested in",
    "into:",
    "hobbies:",
    "topics:",
    "i follow",
    "follows:",
    "cares about",
    "care about",
];

/// Topics from lines like "Interests: Rust, air compressors and xAI".
/// Pulse ledger lines and headings are skipped.
pub fn topics_in_memory(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.contains("] pulse: ") {
            continue;
        }
        let lower = line.to_lowercase();
        let Some((at, marker)) = MEMORY_MARKERS
            .iter()
            .filter_map(|m| lower.find(m).map(|at| (at, *m)))
            .min_by_key(|(at, _)| *at)
        else {
            continue;
        };
        let rest = &line[at + marker.len()..];
        let rest = rest.trim_start_matches([':', ' ']);
        for piece in rest.split([',', ';', '/']).flat_map(|p| p.split(" and ")) {
            let topic = clean_topic(piece);
            let words = topic.split_whitespace().count();
            if (1..=4).contains(&words) && !out.iter().any(|t| t.eq_ignore_ascii_case(&topic)) {
                out.push(topic);
            }
        }
    }
    out
}

/// Chat / Settings chrome titles that are never interests.
const CABIN_TOPIC_BLOCK: &[&str] = &[
    "background",
    "new chat",
    "untitled",
    "chat",
    "scratch",
    "new session",
    "settings",
    "pulse",
    "sources",
    "interests",
    "feed",
    "ideas",
    "cabin",
    "grokhub",
    "history",
    "permissions",
    "labs",
    "behavior",
    "saved finds",
    "follow up",
    "summarize the workboard",
    "background task",
    "background task done",
    "background task failed",
    "background task stopped",
];

/// Single words that are not user interests (English glue + cabin / agent verbs).
const WEAK_UNIGRAMS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "or",
    "of",
    "to",
    "in",
    "on",
    "for",
    "with",
    "from",
    "this",
    "that",
    "it",
    "is",
    "are",
    "was",
    "be",
    "as",
    "at",
    "by",
    "about",
    "into",
    "over",
    "after",
    "before",
    "new",
    "old",
    "more",
    "less",
    "other",
    "some",
    "any",
    "all",
    "my",
    "your",
    "our",
    "their",
    "me",
    "you",
    "we",
    "they",
    "i",
    "do",
    "did",
    "does",
    "not",
    "no",
    "yes",
    "ok",
    "hi",
    "hey",
    "hello",
    "thanks",
    "please",
    "help",
    "fix",
    "test",
    "build",
    "run",
    "stop",
    "start",
    "open",
    "close",
    "save",
    "delete",
    "update",
    "check",
    "review",
    "search",
    "web",
    "news",
    "home",
    "work",
    "task",
    "tasks",
    "job",
    "jobs",
    "agent",
    "model",
    "auto",
    "always",
    "ask",
    "deny",
    "allow",
    "mute",
    "pin",
    "like",
    "skip",
    "card",
    "cards",
    "post",
    "posts",
    "source",
    "sources",
    "interest",
    "interests",
    "setting",
    "settings",
    "pulse",
    "cabin",
    "chat",
    "chats",
    "session",
    "sessions",
    "scratch",
    "untitled",
    "background",
    "triage",
    "classify",
    "redact",
    "summarize",
    "compact",
    "judge",
    "dream",
    "memory",
    "refine",
    "unattended",
    "eval",
    "probe",
    "repair",
    "prepare",
    "skill",
    "skills",
    "plugin",
    "plugins",
    "feed",
    "feeds",
    "idea",
    "ideas",
    "night",
    "nightly",
    "loop",
    "loops",
    "inbox",
    "governor",
    "harness",
    "follow",
    "up",
];

/// A chat or card title short enough to be a topic, else None.
fn short_title(title: &str) -> Option<String> {
    let topic = clean_topic(title);
    let words = topic.split_whitespace().count();
    if !(1..=4).contains(&words) || !topic_searchable(&topic) {
        return None;
    }
    Some(topic)
}

pub fn load(path: &std::path::Path) -> InterestProfile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &std::path::Path, profile: &InterestProfile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Interests: {e}"))?;
    }
    let text = serde_json::to_string_pretty(profile).map_err(|e| format!("Interests: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Interests: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Interests: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn seeding_reads_memory_lines_saved_cards_and_evidence_gated_chats() {
        let user = "# About me\nInterests: Rust, air compressors and xAI.\nWorks nights.";
        let memory = "- [2026-10-09] pulse: liked \"Tokio 2.0\" reason=liked\nI follow Formula 1";
        // One chat is not enough to promote; two chats with the same title are.
        let chats = vec![
            ("c1".into(), "New chat".into()),
            (
                "c2".into(),
                "Fix the cabin build on Windows please now".into(),
            ),
            ("c3".into(), "Home espresso".into()),
        ];
        let saved = titles(&["Bambu printers"]);
        let mut p = InterestProfile::default();
        let n = p.seed(
            &SeedFrom {
                user_md: user,
                memory_md: memory,
                chats: &chats,
                saved_titles: &saved,
            },
            7,
        );
        let got: Vec<(&str, InterestSource, u8)> = p
            .topics
            .iter()
            .map(|t| (t.topic.as_str(), t.source, t.weight))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Rust", InterestSource::Memory, 60),
                ("air compressors", InterestSource::Memory, 60),
                ("xAI", InterestSource::Memory, 60),
                ("Formula 1", InterestSource::Memory, 60),
                ("Bambu printers", InterestSource::Saved, 55),
            ]
        );
        assert_eq!(n, 5);
        assert!(p.find("Home espresso").is_none());
        assert!(p.seeded);
        // Second distinct chat with the same title promotes it.
        p.observe_chat("c4", "Home espresso", 8);
        assert_eq!(p.promote_from_evidence(8), 1);
        assert_eq!(
            p.find("Home espresso").map(|t| t.source),
            Some(InterestSource::Chats)
        );
        assert_eq!(
            p.seed(
                &SeedFrom {
                    user_md: "Interests: Go",
                    memory_md: "",
                    chats: &[],
                    saved_titles: &[]
                },
                9
            ),
            0
        );
        assert!(p.find("Go").is_none());
    }

    #[test]
    fn observe_links_known_topics_and_probe_skips_until_evidence() {
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Memory, 1);
        p.add("async runtimes", InterestSource::Explicit, 1);
        p.observe_chat("a", "Rust and async runtimes", 10);
        assert!(p
            .graph
            .edges
            .iter()
            .any(|e| e.from == "Rust" && e.to == "async runtimes" && e.chat_ids == ["a"]));
        // One co-mention chat is not enough to probe; neighbor is also already a ready interest.
        assert!(p.probe_queries(PROBE_MAX).is_empty());
        // A neighbor that is not itself a ready interest can be probed after two chats.
        p.graph.edges.push(InterestEdge {
            from: "Rust".into(),
            to: "systems programming".into(),
            kind: EdgeKind::Related,
            weight: EDGE_WEIGHT_START,
            chat_ids: vec!["a".into(), "b".into()],
            closed: false,
            last_seen_ms: 11,
        });
        assert_eq!(
            p.probe_queries(PROBE_MAX)
                .iter()
                .map(|r| (r.query.as_str(), r.topic.as_deref()))
                .collect::<Vec<_>>(),
            vec![("systems programming", Some("Rust"))]
        );
        p.feedback_anchored(
            "Why systems programming is hard",
            Some("Rust"),
            InterestSignal::NotInterested,
            12,
        );
        assert!(p
            .graph
            .edges
            .iter()
            .any(|e| e.from == "Rust" && e.to == "systems programming" && e.closed));
        assert!(p
            .probe_queries(PROBE_MAX)
            .iter()
            .all(|r| r.query != "systems programming"));
    }

    #[test]
    fn save_strengthens_a_related_edge() {
        let mut p = InterestProfile::default();
        p.add("photography", InterestSource::Memory, 1);
        p.add("film cameras", InterestSource::Explicit, 1);
        p.observe_chat("a", "photography with film cameras", 1);
        p.observe_chat("b", "film cameras and photography", 2);
        let before = p
            .graph
            .edges
            .iter()
            .find(|e| e.from == "photography" && e.to == "film cameras")
            .map(|e| e.weight)
            .expect("edge");
        p.feedback_anchored(
            "New film cameras for photography",
            Some("photography"),
            InterestSignal::Saved,
            3,
        );
        let after = p
            .graph
            .edges
            .iter()
            .find(|e| e.from == "photography" && e.to == "film cameras")
            .map(|e| e.weight)
            .expect("edge");
        assert_eq!(after, before + 12);
    }

    #[test]
    fn a_removed_topic_stays_gone_until_the_person_adds_it_back() {
        let mut p = InterestProfile::default();
        assert!(p.add("Rust", InterestSource::Memory, 1));
        assert!(p.remove("rust"));
        assert_eq!(p.removed, vec!["rust".to_string()]);
        assert!(!p.add("Rust", InterestSource::Chats, 2));
        assert!(p.add("Rust", InterestSource::Explicit, 3));
        assert_eq!(
            p.find("RUST").map(|t| (t.weight, t.source)),
            Some((70, InterestSource::Explicit))
        );
        assert!(p.removed.is_empty());
    }

    #[test]
    fn feedback_moves_matching_topics_and_spares_pinned_ones() {
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Memory, 0);
        p.add("air compressors", InterestSource::Chats, 0);
        p.add("xAI", InterestSource::Explicit, 0);
        p.set_pinned("xAI", true);
        assert_eq!(
            p.feedback("Rust 1.99 ships async closures", InterestSignal::Saved, 50),
            vec!["Rust"]
        );
        assert_eq!(
            p.find("Rust").map(|t| (t.weight, t.last_seen_ms)),
            Some((70, 50))
        );
        // "trust" is not "Rust".
        assert!(p
            .feedback("Why trust matters", InterestSignal::Saved, 60)
            .is_empty());
        p.feedback(
            "New air compressors for home shops",
            InterestSignal::NotInterested,
            70,
        );
        assert_eq!(p.find("air compressors").map(|t| t.weight), Some(25));
        assert!(p
            .feedback("xAI raises again", InterestSignal::NotInterested, 80)
            .is_empty());
        assert_eq!(
            p.find("xAI").map(|t| (t.weight, t.last_seen_ms)),
            Some((70, 80))
        );
        for _ in 0..10 {
            p.feedback("air compressors", InterestSignal::NotInterested, 90);
        }
        assert_eq!(
            p.find("air compressors").map(|t| t.weight),
            Some(WEIGHT_MIN)
        );
    }

    #[test]
    fn prompt_section_puts_pinned_first_and_names_muted_topics() {
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Memory, 0);
        // Chat-sourced without evidence is not ready for the prompt; pin makes it ready.
        p.add("xAI", InterestSource::Chats, 0);
        p.add("crypto", InterestSource::Memory, 0);
        p.set_pinned("xAI", true);
        p.set_muted("crypto", true);
        assert_eq!(
            p.prompt_section(),
            "Their interests, strongest first: xAI, Rust. Never about: crypto."
        );
        assert_eq!(InterestProfile::default().prompt_section(), "");
        // Pin and mute exclude each other.
        p.set_pinned("crypto", true);
        assert_eq!(
            p.find("crypto").map(|t| (t.pinned, t.muted)),
            Some((true, false))
        );
    }

    #[test]
    fn a_full_profile_drops_its_weakest_learned_topic_for_a_new_one() {
        let mut p = InterestProfile::default();
        for i in 0..TOPICS_MAX {
            p.add(&format!("topic {i}"), InterestSource::Explicit, 0);
        }
        assert!(!p.add("one more", InterestSource::Chats, 1));
        p.topics[3].source = InterestSource::Chats;
        p.topics[3].weight = 10;
        assert!(p.add("one more", InterestSource::Chats, 1));
        assert_eq!(p.topics.len(), TOPICS_MAX);
        assert!(p.find("topic 3").is_none());
    }

    #[test]
    fn clean_topic_trims_bullets_and_cuts_on_a_word() {
        assert_eq!(clean_topic("  - Rust   lang.  "), "Rust lang");
        assert_eq!(
            clean_topic("an extremely long topic name that keeps going and going"),
            "an extremely long topic name that keeps"
        );
    }

    #[test]
    fn search_topics_skip_cabin_jargon_and_weak_unigrams() {
        let mut p = InterestProfile::default();
        // Hidden Background chat title and agent lane names must never query.
        assert!(p.add("Background", InterestSource::Chats, 1));
        assert!(p.add("background:triage", InterestSource::Chats, 1));
        assert!(p.add("triage", InterestSource::Chats, 1));
        assert!(p.add("Settings", InterestSource::Chats, 1));
        assert!(p.add("the", InterestSource::Memory, 1));
        // Weak chat unigram stays off the search list even at full weight.
        assert!(p.add("refine", InterestSource::Chats, 1));
        if let Some(t) = p.find_mut("refine") {
            t.weight = WEIGHT_MAX;
        }
        // Real sealed interests still search.
        assert!(p.add("Rust", InterestSource::Memory, 1));
        assert!(p.add("air compressors", InterestSource::Explicit, 1));
        // Multi-word chat title is fine once evidence-ready (raised weight).
        assert!(p.add("Home espresso", InterestSource::Chats, 1));
        if let Some(t) = p.find_mut("Home espresso") {
            t.weight = InterestSource::Chats.start_weight() + 1;
        }
        assert_eq!(
            p.search_topics(),
            vec![
                "air compressors".to_string(),
                "Rust".to_string(),
                "Home espresso".to_string(),
            ]
        );
        assert!(!topic_searchable("background"));
        assert!(!topic_searchable("background:triage"));
        assert!(!topic_searchable("background:refine"));
        // Real multi-word phrases stay searchable; junk drift comes from weak
        // tokens like "background", not from inventing a fallback query.
        assert!(topic_searchable("Girl Scouts"));
        let mut noise = InterestProfile::default();
        assert!(noise.add("Background", InterestSource::Chats, 1));
        assert!(noise.add("Pulse", InterestSource::Chats, 1));
        assert!(noise.search_topics().is_empty());
    }

    #[test]
    fn seeding_skips_cabin_chrome_titles() {
        let mut p = InterestProfile::default();
        // Two chats with the same good title promote; chrome titles never land.
        let chats = vec![
            ("c1".into(), "Background".into()),
            ("c2".into(), "background:refine".into()),
            ("c3".into(), "New chat".into()),
            ("c4".into(), "Bambu Lab".into()),
            ("c5".into(), "Bambu Lab".into()),
        ];
        let n = p.seed(
            &SeedFrom {
                user_md: "Interests: Rust, triage.",
                memory_md: "",
                chats: &chats,
                saved_titles: &[],
            },
            1,
        );
        let got: Vec<&str> = p.topics.iter().map(|t| t.topic.as_str()).collect();
        assert_eq!(got, vec!["Rust", "Bambu Lab"]);
        assert_eq!(n, 2);
        assert!(p.find("Background").is_none());
        assert!(p.find("triage").is_none());
        assert!(!p
            .graph
            .evidence
            .iter()
            .any(|e| e.topic.eq_ignore_ascii_case("Background")));
    }

    #[test]
    fn the_profile_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("grokhub-interests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE_NAME);
        assert_eq!(load(&path), InterestProfile::default());
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Explicit, 9);
        p.set_muted("Rust", true);
        save(&path, &p).expect("save");
        assert_eq!(load(&path), p);
        std::fs::write(&path, "not json").expect("write");
        assert_eq!(load(&path), InterestProfile::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
