//! Commands that open a page or show cabin state.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    BOARD {
        names: ["/board"],
        effect: Local,
        sample: "/board",
        parse: |_rest| Some(Slash::Board),
    }
}

register_slash! {
    JOURNAL {
        names: ["/journal"],
        effect: Local,
        sample: "/journal",
        parse: |_rest| Some(Slash::Journal),
    }
}

register_slash! {
    WORKTREE {
        names: ["/worktree"],
        effect: Local,
        sample: "/worktree",
        parse: |_rest| Some(Slash::Worktree),
    }
}

register_slash! {
    WORKFLOWS {
        names: ["/workflows"],
        effect: Local,
        sample: "/workflows",
        parse: |_rest| Some(Slash::GrokWorkflows),
    }
}

register_slash! {
    PLUGINS {
        names: ["/plugins", "/marketplace", "/mcps", "/connectors"],
        effect: Local,
        sample: "/connectors",
        parse: |_rest| Some(Slash::GrokConnectors),
    }
}

register_slash! {
    HOOKS {
        names: ["/hooks"],
        effect: Local,
        sample: "/hooks",
        parse: |_rest| Some(Slash::GrokHooks),
    }
}

register_slash! {
    DASHBOARD {
        names: ["/dashboard", "/agents-dashboard"],
        effect: Local,
        sample: "/dashboard",
        parse: |_rest| Some(Slash::Sessions),
    }
}

register_slash! {
    STATUS {
        names: ["/status", "/info", "/session-info", "/doctor"],
        effect: Local,
        sample: "/status",
        parse: |_rest| Some(Slash::Inspect),
    }
}

register_slash! {
    LEARN {
        names: ["/learn"],
        effect: Local,
        sample: "/learn",
        parse: |_rest| Some(Slash::LearnReflect),
    }
}

register_slash! {
    HOST {
        names: ["/host", "/tools"],
        effect: Local,
        sample: "/host",
        parse: |_rest| Some(Slash::HostStatus),
    }
}

register_slash! {
    HEALTH {
        names: ["/health"],
        effect: Local,
        sample: "/health",
        parse: |_rest| Some(Slash::Health),
    }
}

register_slash! {
    BRIEF {
        names: ["/brief"],
        effect: Local,
        sample: "/brief",
        parse: |_rest| Some(Slash::Brief),
    }
}

register_slash! {
    FIX {
        names: ["/fix"],
        effect: Local,
        sample: "/fix",
        parse: |_rest| Some(Slash::Fix),
    }
}

register_slash! {
    DREAM {
        names: ["/dream"],
        effect: Local,
        sample: "/dream",
        parse: |_rest| Some(Slash::Dream),
    }
}

register_slash! {
    IMPORT {
        names: ["/import"],
        effect: Local,
        sample: "/import",
        parse: |_rest| Some(Slash::Import),
    }
}

register_slash! {
    USAGE {
        names: ["/usage"],
        effect: Local,
        sample: "/usage",
        parse: |_rest| Some(Slash::Usage),
    }
}

register_slash! {
    MODELS {
        names: ["/models"],
        effect: Local,
        sample: "/models",
        parse: |_rest| Some(Slash::Models),
    }
}

register_slash! {
    PALETTE {
        names: ["/palette"],
        effect: Local,
        sample: "/palette",
        parse: |_rest| Some(Slash::Palette),
    }
}

register_slash! {
    EFFORT {
        names: ["/effort"],
        effect: Local,
        sample: "/effort",
        parse: |_rest| Some(Slash::Effort),
    }
}

register_slash! {
    SESSIONS {
        names: ["/sessions", "/resume"],
        effect: Local,
        sample: "/sessions",
        parse: |_rest| Some(Slash::Sessions),
    }
}

register_slash! {
    INSPECT {
        names: ["/inspect"],
        effect: Local,
        sample: "/inspect",
        parse: |_rest| Some(Slash::Inspect),
    }
}

register_slash! {
    PRIVACY {
        names: ["/privacy"],
        effect: Local,
        sample: "/privacy",
        parse: |_rest| Some(Slash::Privacy),
    }
}

register_slash! {
    DIAGNOSE {
        names: ["/diagnose"],
        effect: Local,
        sample: "/diagnose",
        parse: |_rest| Some(Slash::Diagnose),
    }
}

register_slash! {
    HUB {
        names: ["/hub"],
        effect: Local,
        sample: "/hub",
        parse: |_rest| Some(Slash::Hub),
    }
}
