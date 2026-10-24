//! Looking back: `/why`, `/rewind`, `/export`.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    /// `/why`, `/why models`, `/why table`; any other word parses to nothing.
    WHY {
        names: ["/why"],
        effect: Local,
        sample: "/why models",
        parse: |rest| {
            if rest.is_empty() {
                Some(Slash::Why)
            } else if rest.eq_ignore_ascii_case("models") {
                Some(Slash::WhyModels)
            } else if rest.eq_ignore_ascii_case("table") {
                Some(Slash::WhyTable)
            } else {
                None
            }
        },
    }
}

register_slash! {
    /// `/rewind files`, `/rewind step [n]`, or bare `/rewind`.
    REWIND {
        names: ["/rewind"],
        effect: Local,
        sample: "/rewind step 2",
        parse: |rest| {
            if rest == "--files" || rest == "--code" || rest == "files" {
                return Some(Slash::RewindFiles);
            }
            if rest == "step" || rest.starts_with("step ") {
                let arg = rest.strip_prefix("step").unwrap_or("").trim();
                return Some(Slash::RewindStep(arg.parse::<u64>().ok()));
            }
            Some(Slash::Rewind)
        },
    }
}

register_slash! {
    EXPORT {
        names: ["/export"],
        effect: Local,
        sample: "/export md",
        parse: |rest| {
            if rest.is_empty() {
                Some(Slash::Export)
            } else {
                Some(Slash::ExportAs(rest.to_string()))
            }
        },
    }
}
