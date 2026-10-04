// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Permission rule strings. Deny, ask, and allow share one parser.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
    Ask,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Any,
    Bash,
    Read,
    Edit,
    Grep,
    Mcp,
    WebFetch,
    WebSearch,
    AgentMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatMode {
    Glob,
    Domain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub action: Action,
    pub source: String,
    pub tool: Tool,
    pub pattern: Option<String>,
    pub mode: PatMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleParseError {
    UnsupportedToolPrefix { prefix: String },
    UnknownToolPrefix { prefix: String },
    MalformedRule { detail: String },
}

impl fmt::Display for RuleParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedToolPrefix { prefix } => {
                write!(f, "unsupported tool prefix: {prefix}")
            }
            Self::UnknownToolPrefix { prefix } => write!(f, "unknown tool prefix: {prefix}"),
            Self::MalformedRule { detail } => write!(f, "malformed rule: {detail}"),
        }
    }
}

impl std::error::Error for RuleParseError {}

pub fn parse_rule(rule: &str, action: Action) -> Result<Rule, RuleParseError> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err(RuleParseError::MalformedRule {
            detail: "empty rule".into(),
        });
    }
    if rule.len() > 500 {
        return Err(RuleParseError::MalformedRule {
            detail: "rule is too long".into(),
        });
    }
    let source = rule.to_string();
    if let Some(open_paren) = find_first_unescaped(rule, b'(') {
        let prefix = rule
            .get(..open_paren)
            .ok_or_else(|| RuleParseError::MalformedRule {
                detail: "missing opening parenthesis".into(),
            })?;
        let prefix_trimmed = prefix.trim();
        let content_and_close =
            rule.get(open_paren + 1..)
                .ok_or_else(|| RuleParseError::MalformedRule {
                    detail: "missing closing parenthesis".into(),
                })?;
        let close_paren = find_last_unescaped(content_and_close, b')').ok_or_else(|| {
            RuleParseError::MalformedRule {
                detail: "missing closing parenthesis".into(),
            }
        })?;
        let raw_content =
            content_and_close
                .get(..close_paren)
                .ok_or_else(|| RuleParseError::MalformedRule {
                    detail: "missing closing parenthesis".into(),
                })?;
        let raw_content = raw_content.trim();
        let pattern = if raw_content.is_empty() || raw_content == "*" {
            String::new()
        } else {
            unescape_rule_content(raw_content)
        };
        let tool = match tool_name(prefix_trimmed) {
            Some(tool) => tool,
            None if matches!(
                prefix_trimmed,
                "EnterWorktree" | "NotebookEdit" | "NotebookRead"
            ) =>
            {
                return Err(RuleParseError::UnsupportedToolPrefix {
                    prefix: prefix_trimmed.to_string(),
                });
            }
            None => {
                return Err(RuleParseError::UnknownToolPrefix {
                    prefix: prefix_trimmed.to_string(),
                });
            }
        };
        let pattern = if tool == Tool::Bash {
            strip_bash_colon_wildcard(pattern)
        } else {
            pattern
        };
        let (pattern, mode) = strip_domain_prefix(pattern);
        let pattern = if pattern.is_empty() {
            None
        } else {
            Some(pattern)
        };
        return Ok(Rule {
            action,
            source,
            tool,
            pattern,
            mode,
        });
    }
    if matches!(rule, "EnterWorktree" | "NotebookEdit" | "NotebookRead") {
        return Err(RuleParseError::UnsupportedToolPrefix {
            prefix: rule.to_string(),
        });
    }
    if let Some(tool) = tool_name(rule) {
        return Ok(Rule {
            action,
            source,
            tool,
            pattern: None,
            mode: PatMode::Glob,
        });
    }
    if let Some(rest) = rule.strip_prefix("mcp__") {
        if !rest.is_empty() {
            let pattern = if rest == "*" {
                None
            } else if rest.contains("__") {
                Some(rest.to_string())
            } else {
                Some(format!("{rest}__*"))
            };
            return Ok(Rule {
                action,
                source,
                tool: Tool::Mcp,
                pattern,
                mode: PatMode::Glob,
            });
        }
    }
    Ok(Rule {
        action,
        source,
        tool: Tool::Any,
        pattern: Some(rule.to_string()),
        mode: PatMode::Glob,
    })
}

fn tool_name(name: &str) -> Option<Tool> {
    match name {
        "Bash" => Some(Tool::Bash),
        "Read" => Some(Tool::Read),
        "Edit" | "Write" => Some(Tool::Edit),
        "MCPTool" => Some(Tool::Mcp),
        "Grep" | "Glob" => Some(Tool::Grep),
        "WebFetch" => Some(Tool::WebFetch),
        "WebSearch" => Some(Tool::WebSearch),
        "AgentMessage" | "SendSubagentMessage" | "SendAgentMessage" => Some(Tool::AgentMessage),
        _ => None,
    }
}

fn is_unescaped(bytes: &[u8], pos: usize) -> bool {
    let mut backslashes = 0usize;
    let mut j = pos;
    while j > 0 && bytes.get(j - 1) == Some(&b'\\') {
        backslashes += 1;
        j -= 1;
    }
    backslashes.is_multiple_of(2)
}

fn find_first_unescaped(s: &str, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    bytes
        .iter()
        .enumerate()
        .find(|&(i, &b)| b == target && is_unescaped(bytes, i))
        .map(|(i, _)| i)
}

fn find_last_unescaped(s: &str, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    (0..bytes.len())
        .rev()
        .find(|&i| bytes.get(i) == Some(&target) && is_unescaped(bytes, i))
}

fn unescape_rule_content(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    s.replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\\\", "\\")
}

fn strip_domain_prefix(pattern: String) -> (String, PatMode) {
    match pattern.strip_prefix("domain:") {
        Some(domain) => (domain.to_string(), PatMode::Domain),
        None => (pattern, PatMode::Glob),
    }
}

/// A trailing `:*` is a prefix (`Bash(git commit:*)` → `git commit`). A `:*` elsewhere stays literal.
fn strip_bash_colon_wildcard(pattern: String) -> String {
    match pattern.strip_suffix(":*") {
        Some(prefix) => prefix.to_string(),
        None => pattern,
    }
}
