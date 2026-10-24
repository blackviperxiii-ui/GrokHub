//! Model and speed picks.

use crate::slash::registry::register_slash;
use crate::slash::{resolve_mode_arg, Slash};

register_slash! {
    MODEL {
        names: ["/model", "/m"],
        effect: Local,
        sample: "/model grok-4.7",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Model(rest.to_string())),
    }
}

register_slash! {
    MODE {
        names: ["/mode"],
        effect: Local,
        sample: "/mode fast",
        parse: |rest| {
            if rest.is_empty() {
                return None;
            }
            resolve_mode_arg(rest).map(Slash::Mode)
        },
    }
}
