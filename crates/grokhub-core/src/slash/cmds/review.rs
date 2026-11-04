//! Looking back: `/rewind`, `/export`.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

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
