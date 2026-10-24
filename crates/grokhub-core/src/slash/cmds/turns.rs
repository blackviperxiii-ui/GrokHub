//! Commands that start model work or write a note: Imagine, loops, skills, consult, send.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    RECALL {
        names: ["/recall"],
        effect: Local,
        sample: "/recall pi",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Recall(rest.to_string())),
    }
}

register_slash! {
    IMAGINE {
        names: ["/imagine"],
        effect: AgentTurn,
        sample: "/imagine a lighthouse",
        parse: |rest| Some(Slash::Imagine(rest.to_string())),
    }
}

register_slash! {
    IMAGINE_VIDEO {
        names: ["/imagine-video"],
        effect: AgentTurn,
        sample: "/imagine-video waves",
        parse: |rest| Some(Slash::ImagineVideo(rest.to_string())),
    }
}

register_slash! {
    LOOP {
        names: ["/loop"],
        effect: AgentTurn,
        sample: "/loop check mail",
        parse: |rest| Some(Slash::Loop(rest.to_string())),
    }
}

register_slash! {
    GOAL {
        names: ["/goal"],
        effect: Local,
        sample: "/goal ship it",
        parse: |rest| Some(Slash::Goal(rest.to_string())),
    }
}

register_slash! {
    SKILL {
        names: ["/skill"],
        effect: AgentTurn,
        sample: "/skill tidy",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Skill(rest.to_string())),
    }
}

register_slash! {
    REMEMBER {
        names: ["/remember"],
        effect: Local,
        sample: "/remember tabs over spaces",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Remember(rest.to_string())),
    }
}

register_slash! {
    CONSULT {
        names: ["/consult"],
        effect: AgentTurn,
        sample: "/consult is this safe",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Consult(rest.to_string())),
    }
}

register_slash! {
    SEND {
        names: ["/send"],
        effect: AgentTurn,
        sample: "/send check the logs",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Send(rest.to_string())),
    }
}

register_slash! {
    AUDIOCHECK {
        names: ["/audiocheck"],
        effect: Local,
        sample: "/audiocheck",
        parse: |rest| Some(Slash::AudioCheck(rest.to_string())),
    }
}

register_slash! {
    ROOM {
        names: ["/room"],
        effect: Local,
        sample: "/room kitchen",
        parse: |rest| (!rest.is_empty()).then(|| Slash::Room(rest.to_string())),
    }
}
