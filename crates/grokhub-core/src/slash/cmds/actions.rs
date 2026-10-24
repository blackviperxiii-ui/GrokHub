//! Commands that run or unlock something. Each one is an `Action` with a gate row
//! in `crates/grokhub-app/src/app/slash_gate_tests.rs`.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    UPDATE {
        names: ["/update"],
        effect: Action,
        sample: "/update",
        parse: |_rest| Some(Slash::Update),
    }
}

register_slash! {
    SH {
        names: ["/sh"],
        effect: Action,
        sample: "/sh ls",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Sh(rest.to_string())),
    }
}

register_slash! {
    PLAN {
        names: ["/plan"],
        effect: Local,
        sample: "/plan",
        parse: |_rest| Some(Slash::Plan),
    }
}

register_slash! {
    ALWAYS_APPROVE {
        names: ["/always-approve", "/yolo"],
        effect: Action,
        sample: "/always-approve",
        parse: |_rest| Some(Slash::AlwaysApprove),
    }
}

register_slash! {
    AUTO {
        names: ["/auto"],
        effect: Action,
        sample: "/auto",
        parse: |_rest| Some(Slash::AutoPerm),
    }
}

register_slash! {
    SYNC {
        names: ["/sync"],
        effect: Action,
        sample: "/sync",
        parse: |_rest| Some(Slash::Sync),
    }
}

register_slash! {
    INHABIT {
        names: ["/inhabit"],
        effect: Action,
        sample: "/inhabit desk",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Inhabit(rest.to_string())),
    }
}

register_slash! {
    /// `/record stop` ends a recording and is `Local`; anything else starts one.
    RECORD {
        names: ["/record"],
        effect: Action,
        sample: "/record the menu flickers",
        parse: |rest| {
            if rest.eq_ignore_ascii_case("stop") {
                Some(Slash::RecordStop)
            } else {
                Some(Slash::Record(rest.to_string()))
            }
        },
    }
}
