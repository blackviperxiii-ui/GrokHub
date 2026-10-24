//! Chat thread commands: new, clear, retry, rename and the like.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    BTW {
        names: ["/btw"],
        effect: Local,
        sample: "/btw",
        parse: |_rest| Some(Slash::Btw),
    }
}

register_slash! {
    BG {
        names: ["/bg", "/background"],
        effect: AgentTurn,
        sample: "/bg tidy the docs",
        parse: |rest| Some(Slash::Background(rest.to_string())),
    }
}

register_slash! {
    QUEUE {
        names: ["/queue"],
        effect: AgentTurn,
        sample: "/queue and then this",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Queue(rest.to_string())),
    }
}

register_slash! {
    VIEW_PLAN {
        names: ["/view-plan", "/show-plan", "/plan-view"],
        effect: Local,
        sample: "/view-plan",
        parse: |_rest| Some(Slash::ViewPlan),
    }
}

register_slash! {
    TITLE {
        names: ["/title"],
        effect: Local,
        sample: "/title x",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Rename(rest.to_string())),
    }
}

register_slash! {
    COMPACT {
        names: ["/compact"],
        effect: AgentTurn,
        sample: "/compact",
        parse: |_rest| Some(Slash::Compact),
    }
}

register_slash! {
    NEW {
        names: ["/new"],
        effect: Local,
        sample: "/new",
        parse: |_rest| Some(Slash::New),
    }
}

register_slash! {
    SCRATCH {
        names: ["/scratch"],
        effect: Local,
        sample: "/scratch",
        parse: |_rest| Some(Slash::Scratch),
    }
}

register_slash! {
    CLEAR {
        names: ["/clear"],
        effect: Local,
        sample: "/clear",
        parse: |_rest| Some(Slash::Clear),
    }
}

register_slash! {
    UNDO {
        names: ["/undo"],
        effect: Local,
        sample: "/undo",
        parse: |_rest| Some(Slash::Undo),
    }
}

register_slash! {
    RETRY {
        names: ["/retry"],
        effect: AgentTurn,
        sample: "/retry",
        parse: |_rest| Some(Slash::Retry),
    }
}

register_slash! {
    STOP {
        names: ["/stop"],
        effect: Local,
        sample: "/stop",
        parse: |_rest| Some(Slash::Stop),
    }
}

register_slash! {
    RENAME {
        names: ["/rename"],
        effect: Local,
        sample: "/rename Pier light",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Rename(rest.to_string())),
    }
}

register_slash! {
    PIN {
        names: ["/pin"],
        effect: Local,
        sample: "/pin",
        parse: |_rest| Some(Slash::Pin),
    }
}

register_slash! {
    DELETE {
        names: ["/delete", "/close"],
        effect: Local,
        sample: "/delete",
        parse: |_rest| Some(Slash::Delete),
    }
}

register_slash! {
    CONTEXT {
        names: ["/context"],
        effect: Local,
        sample: "/context",
        parse: |_rest| Some(Slash::Context),
    }
}
