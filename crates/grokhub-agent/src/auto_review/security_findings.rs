// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! A finding is a fixed enum token naming a static-analysis risk the built-in Bash gate detected, plus a fixed harness-owned description.
//! Both are static constants that never carry a command, path, or argument; those stay in the untrusted proposed action.
//! An attacker therefore cannot steer the classifier by smuggling text through a finding.
//! Nonempty findings force the model path (the heuristic pre-pass may not auto-Allow), so the classifier always sees them before approving.

use std::collections::BTreeSet;

/// One built-in Bash security finding; each variant renders to a single stable wire token.
/// `Ord` fixes the canonical render order (variant order below), so [`BashSecurityAssessment`] renders deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClassifierSecurityFinding {
    /// Managed-policy analysis failed closed with no explicit rule match.
    #[cfg_attr(not(test), allow(dead_code))]
    FailClosedPolicy,
    /// Shell structure the parser could not decompose to check.
    UnparseableShell,
    /// Argument values depend on shell expansion.
    #[cfg_attr(not(test), allow(dead_code))]
    UnresolvedArgument,
    /// Nested/opaque shell execution (`bash -c "$X"`, `eval …`).
    #[cfg_attr(not(test), allow(dead_code))]
    OpaqueShell,
    /// Execution callback or ambient Git-config exec risk.
    #[cfg_attr(not(test), allow(dead_code))]
    ExecOrAmbientGit,
    /// Environment injection key (LD_PRELOAD, DYLD_*, GIT_CONFIG*, …).
    EnvInjection,
    /// Environment assignment outside the cosmetic-safe allowlist.
    UnvettedEnv,
    /// Write to a real (non-sink) file.
    #[cfg_attr(not(test), allow(dead_code))]
    FileWrite,
    /// Dangerous command segment (`rm`, `chmod`, `git push`, …).
    DangerousCommand,
    /// Special executable/disclosure surface (`rg` unsafe flags, Git drivers/pagers, kubectl config/auth overrides, process-environment dump).
    #[cfg_attr(not(test), allow(dead_code))]
    SpecialExecSurface,
}

impl ClassifierSecurityFinding {
    /// Every finding variant, in canonical order.
    #[cfg(test)]
    pub const ALL: &'static [Self] = &[
        Self::FailClosedPolicy,
        Self::UnparseableShell,
        Self::UnresolvedArgument,
        Self::OpaqueShell,
        Self::ExecOrAmbientGit,
        Self::EnvInjection,
        Self::UnvettedEnv,
        Self::FileWrite,
        Self::DangerousCommand,
        Self::SpecialExecSurface,
    ];

    /// Stable wire token rendered into the classifier system message.
    /// Never changes without a matching classifier-prompt update.
    pub const fn token(self) -> &'static str {
        match self {
            Self::FailClosedPolicy => "fail_closed_policy",
            Self::UnparseableShell => "unparseable_shell",
            Self::UnresolvedArgument => "unresolved_argument",
            Self::OpaqueShell => "opaque_shell",
            Self::ExecOrAmbientGit => "exec_or_ambient_git",
            Self::EnvInjection => "env_injection",
            Self::UnvettedEnv => "unvetted_env",
            Self::FileWrite => "file_write",
            Self::DangerousCommand => "dangerous_command",
            Self::SpecialExecSurface => "special_exec_surface",
        }
    }

    /// Fixed, harness-owned explanation of the token.
    /// Like [`token`](Self::token) it is a static string carrying no command, path, argument, or user text, so it cannot inject instructions.
    pub const fn description(self) -> &'static str {
        match self {
            Self::FailClosedPolicy => {
                "permission policy could not determine whether a rule applies"
            }
            Self::UnparseableShell => "shell structure could not be fully parsed",
            Self::UnresolvedArgument => "argument values depend on shell expansion",
            Self::OpaqueShell => "invokes a nested or dynamically supplied shell command",
            Self::ExecOrAmbientGit => {
                "may run code via an execution callback or ambient Git configuration"
            }
            Self::EnvInjection => "sets environment variables that can change which code executes",
            Self::UnvettedEnv => "sets environment variables outside the known-safe set",
            Self::FileWrite => "writes to a real file rather than a sink",
            Self::DangerousCommand => "a destructive or high-impact command",
            Self::SpecialExecSurface => {
                "options or tools that can write arbitrary output, execute code, override config/identity, or disclose the process environment"
            }
        }
    }

    /// Whether this finding constrains a broad grant (blanket execute, prefix/glob, sandbox auto-allow).
    /// A broad grant cannot vouch for these effects, so they must reach the classifier rather than auto-allow.
    /// `DangerousCommand`, `UnparseableShell`, and `FailClosedPolicy` are handled by their own arms.
    #[cfg(test)]
    const fn is_grant_floor(self) -> bool {
        matches!(
            self,
            Self::FileWrite
                | Self::UnvettedEnv
                | Self::EnvInjection
                | Self::OpaqueShell
                | Self::ExecOrAmbientGit
                | Self::SpecialExecSurface
        )
    }
}

/// Canonical ordered, deduplicated finding set for one request; `BTreeSet` encodes that invariant so callers cannot reorder or duplicate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BashSecurityAssessment(BTreeSet<ClassifierSecurityFinding>);

impl BashSecurityAssessment {
    pub(crate) fn insert(&mut self, finding: ClassifierSecurityFinding) {
        self.0.insert(finding);
    }

    /// No findings: the command is fully safe by static analysis.
    /// A broad policy Allow or fast path may only skip the classifier when this holds.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the given finding is present.
    #[cfg(test)]
    pub fn contains(&self, finding: ClassifierSecurityFinding) -> bool {
        self.0.contains(&finding)
    }

    /// Whether any finding constrains a broad grant or sandbox auto-allow.
    #[cfg(test)]
    pub(crate) fn constrains_broad_grant(&self) -> bool {
        self.0
            .iter()
            .copied()
            .any(ClassifierSecurityFinding::is_grant_floor)
    }

    /// Compact `[token, token]` list in canonical order, for tests to pin the ordered/deduplicated invariant.
    #[cfg(test)]
    pub(crate) fn render_tokens(&self) -> String {
        let tokens: Vec<&str> = self.0.iter().map(|finding| finding.token()).collect();
        format!("[{}]", tokens.join(", "))
    }

    /// One `- token: description` line per present finding, in canonical order.
    pub(crate) fn render_glossary(&self) -> String {
        self.0
            .iter()
            .map(|finding| format!("- {}: {}", finding.token(), finding.description()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl FromIterator<ClassifierSecurityFinding> for BashSecurityAssessment {
    fn from_iter<I: IntoIterator<Item = ClassifierSecurityFinding>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
#[path = "security_findings_tests.rs"]
mod tests;
