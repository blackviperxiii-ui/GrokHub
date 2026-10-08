//! Node markdown and edge JSON. Callers supply `created` and `updated`.

use super::AmrError;

/// `amr_schema` written into `amr/README.md`.
pub const AMR_SCHEMA: u32 = 1;

/// The node kinds. Files use the lowercase names. Spike-5a added the user
/// model kinds (`routine`, `need`, `mind_prior`) after the first six.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Preference,
    Fact,
    Decision,
    Trail,
    Person,
    Project,
    Routine,
    Need,
    MindPrior,
}

impl NodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Fact => "fact",
            Self::Decision => "decision",
            Self::Trail => "trail",
            Self::Person => "person",
            Self::Project => "project",
            Self::Routine => "routine",
            Self::Need => "need",
            Self::MindPrior => "mind_prior",
        }
    }

    /// The user model kinds `/memory` lists (Spike-5a).
    pub fn is_user_model(self) -> bool {
        matches!(self, Self::Fact | Self::Preference | Self::Routine | Self::Need | Self::MindPrior)
    }

    pub fn parse(raw: &str) -> Result<Self, AmrError> {
        match raw {
            "preference" => Ok(Self::Preference),
            "fact" => Ok(Self::Fact),
            "decision" => Ok(Self::Decision),
            "trail" => Ok(Self::Trail),
            "person" => Ok(Self::Person),
            "project" => Ok(Self::Project),
            "routine" => Ok(Self::Routine),
            "need" => Ok(Self::Need),
            "mind_prior" => Ok(Self::MindPrior),
            other => Err(AmrError::UnknownType(other.to_string())),
        }
    }
}

/// One fact file. `id` is the filename stem `nodes/<id>.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub node_type: NodeType,
    pub created: String,
    pub updated: String,
    pub source: String,
    pub confidence: f32,
    pub tags: Vec<String>,
    pub body: String,
    /// The consent grant behind a `scope:` node. Empty when none applied;
    /// written only when set, so older nodes read back unchanged.
    pub consent_ref: String,
    /// Written only when not plain. A `.md` file is always plain.
    pub sensitivity: Sensitivity,
}

/// A node to write. `remember` redacts `source`, `tags`, and `body`.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeDraft {
    pub id: String,
    pub node_type: NodeType,
    pub created: String,
    pub updated: String,
    pub source: String,
    pub confidence: f32,
    pub tags: Vec<String>,
    pub body: String,
    /// Which tier the file lands in (harness design §12 P1/P4).
    pub sensitivity: Sensitivity,
    /// The consent grant id for a `scope:<grant>` source, else empty.
    pub consent_ref: String,
}

/// The tier of a learned node. Plain stays readable markdown; personal and
/// sensitive are sealed at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    Plain,
    Personal,
    Sensitive,
}

impl Sensitivity {
    pub fn sealed(self) -> bool {
        !matches!(self, Self::Plain)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Personal => "personal",
            Self::Sensitive => "sensitive",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, AmrError> {
        match raw {
            "plain" => Ok(Self::Plain),
            "personal" => Ok(Self::Personal),
            "sensitive" => Ok(Self::Sensitive),
            other => Err(AmrError::BadFrontmatter(format!("sensitivity {other}"))),
        }
    }
}

/// Validated node id. `[a-z0-9-]`, 1..=96 bytes, not starting with `-`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeId(String);

impl NodeId {
    pub fn parse(raw: &str) -> Result<Self, AmrError> {
        if raw.is_empty()
            || raw.len() > 96
            || raw.starts_with('-')
            || !raw
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(AmrError::BadId(raw.to_string()));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One `/recall` row before it becomes `amr:<id>: <line>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeHit {
    pub id: String,
    pub line: String,
}

impl NodeHit {
    pub fn display(&self) -> String {
        format!("amr:{}: {}", self.id, self.line)
    }
}

/// `from` and `to` are node ids. `rel` is one of the five schema relations.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub rel: EdgeRel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRel {
    References,
    Supersedes,
    LearnedFromSpan,
    AboutProject,
    Contradicts,
}

impl Node {
    /// Canonical markdown. `to_markdown(from_markdown(s)) == s` for this shape.
    /// `consent_ref` and `sensitivity` lines appear only when set.
    pub fn to_markdown(&self) -> String {
        let mut extra = String::new();
        if !self.consent_ref.is_empty() {
            extra.push_str(&format!("consent_ref: {}\n", self.consent_ref));
        }
        if self.sensitivity.sealed() {
            extra.push_str(&format!("sensitivity: {}\n", self.sensitivity.as_str()));
        }
        format!(
            "---\nid: {id}\ntype: {ty}\ncreated: {created}\nupdated: {updated}\nsource: {source}\nconfidence: {confidence}\ntags: {tags}\n{extra}---\n{body}",
            id = self.id,
            ty = self.node_type.as_str(),
            created = self.created,
            updated = self.updated,
            source = self.source,
            confidence = format_confidence(self.confidence),
            tags = format_tags(&self.tags),
            body = self.body,
        )
    }

    pub fn from_markdown(text: &str) -> Result<Self, AmrError> {
        let rest = text
            .strip_prefix("---\n")
            .ok_or(AmrError::MissingFrontmatter)?;
        let mut offset = 0;
        let mut body_at = None;
        for line in rest.split_inclusive('\n') {
            let bare = line.strip_suffix('\n').unwrap_or(line);
            if bare == "---" {
                body_at = Some(offset + line.len());
                break;
            }
            offset += line.len();
        }
        let body_at = body_at.ok_or(AmrError::MissingFrontmatter)?;
        let front = &rest[..offset];
        let body = &rest[body_at..];
        parse_frontmatter(front, body)
    }
}

pub(super) fn check_confidence(confidence: f32) -> Result<(), AmrError> {
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(AmrError::ConfidenceOutOfRange);
    }
    Ok(())
}

fn format_confidence(confidence: f32) -> String {
    let rendered = format!("{confidence}");
    let same = rendered
        .parse::<f32>()
        .ok()
        .is_some_and(|parsed| parsed.to_bits() == confidence.to_bits());
    if same {
        rendered
    } else {
        format!("{confidence:.8}")
    }
}

fn format_tags(tags: &[String]) -> String {
    let mut out = String::from("[");
    for (i, tag) in tags.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format_tag(tag));
    }
    out.push(']');
    out
}

fn format_tag(tag: &str) -> String {
    let bare = !tag.is_empty() && !tag.contains([' ', ',', '[', ']', '"', '\\', '\n', '\r']);
    if bare {
        return tag.to_string();
    }
    let mut out = String::from("\"");
    for ch in tag.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn parse_frontmatter(front: &str, body: &str) -> Result<Node, AmrError> {
    let mut id = None;
    let mut node_type = None;
    let mut created = None;
    let mut updated = None;
    let mut source = None;
    let mut confidence = None;
    let mut tags = None;
    let mut consent_ref = None;
    let mut sensitivity = None;
    for line in front.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| AmrError::BadFrontmatter(line.to_string()))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "id" => set_once(&mut id, value.to_string())?,
            "type" => set_once(&mut node_type, NodeType::parse(value)?)?,
            "created" => set_once(&mut created, value.to_string())?,
            "updated" => set_once(&mut updated, value.to_string())?,
            "source" => set_once(&mut source, value.to_string())?,
            "confidence" => set_once(&mut confidence, parse_confidence(value)?)?,
            "tags" => set_once(&mut tags, parse_tags(value)?)?,
            "consent_ref" => set_once(&mut consent_ref, value.to_string())?,
            "sensitivity" => set_once(&mut sensitivity, Sensitivity::parse(value)?)?,
            other => return Err(AmrError::BadFrontmatter(other.to_string())),
        }
    }
    let id = id.ok_or_else(|| AmrError::BadFrontmatter("id".into()))?;
    NodeId::parse(&id)?;
    Ok(Node {
        id,
        node_type: node_type.ok_or_else(|| AmrError::BadFrontmatter("type".into()))?,
        created: created.ok_or_else(|| AmrError::BadFrontmatter("created".into()))?,
        updated: updated.ok_or_else(|| AmrError::BadFrontmatter("updated".into()))?,
        source: source.ok_or_else(|| AmrError::BadFrontmatter("source".into()))?,
        confidence: confidence.ok_or(AmrError::ConfidenceOutOfRange)?,
        tags: tags.ok_or_else(|| AmrError::BadFrontmatter("tags".into()))?,
        body: body.to_string(),
        consent_ref: consent_ref.unwrap_or_default(),
        sensitivity: sensitivity.unwrap_or(Sensitivity::Plain),
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), AmrError> {
    if slot.is_some() {
        return Err(AmrError::BadFrontmatter("duplicate key".into()));
    }
    *slot = Some(value);
    Ok(())
}

fn parse_confidence(raw: &str) -> Result<f32, AmrError> {
    let confidence = raw
        .parse::<f32>()
        .map_err(|_| AmrError::ConfidenceOutOfRange)?;
    check_confidence(confidence)?;
    Ok(confidence)
}

fn parse_tags(raw: &str) -> Result<Vec<String>, AmrError> {
    let raw = raw.trim();
    let Some(mut rest) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Err(AmrError::BadFrontmatter("tags".into()));
    };
    if rest.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut tags = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return Err(AmrError::BadFrontmatter("tags".into()));
        }
        let (tag, next) = if let Some(quoted) = rest.strip_prefix('"') {
            take_quoted(quoted)?
        } else {
            take_bare(rest)
        };
        if tag.is_empty() {
            return Err(AmrError::BadFrontmatter("empty tag".into()));
        }
        tags.push(tag);
        rest = next.trim_start();
        if rest.is_empty() {
            break;
        }
        let Some(after) = rest.strip_prefix(',') else {
            return Err(AmrError::BadFrontmatter("tags".into()));
        };
        rest = after;
        if rest.trim().is_empty() {
            return Err(AmrError::BadFrontmatter("tags".into()));
        }
    }
    Ok(tags)
}

fn take_bare(raw: &str) -> (String, &str) {
    let end = raw.find(',').unwrap_or(raw.len());
    let (token, next) = raw.split_at(end);
    (token.trim().to_string(), next)
}

fn take_quoted(raw: &str) -> Result<(String, &str), AmrError> {
    let mut out = String::new();
    let mut chars = raw.char_indices();
    while let Some((idx, ch)) = chars.next() {
        if ch == '\\' {
            let Some((_, next)) = chars.next() else {
                return Err(AmrError::BadFrontmatter("tags".into()));
            };
            out.push(next);
            continue;
        }
        if ch == '"' {
            return Ok((out, &raw[idx + 1..]));
        }
        out.push(ch);
    }
    Err(AmrError::BadFrontmatter("tags".into()))
}
