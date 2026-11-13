//! The cabin's standing rules for the model: who it is on this computer and
//! the lines it may emit. Nothing about the Workboard (KB-04).

/// Stable cabin safety/approval text for the prompt cache prefix.
/// Empty desktop flag stays byte-identical to `CABIN_DESKTOP_RULES`.
pub fn cabin_static_rules(desktop: bool) -> String {
    if !desktop {
        return (*CABIN_DESKTOP_RULES).to_string();
    }
    format!("{}\n{DESKTOP_CABIN_LINE}", *CABIN_DESKTOP_RULES)
}

/// Volatile learned brief for the prompt tail. Empty when there is nothing learned.
pub fn cabin_learned_tail(learned: &str) -> String {
    let learned: String = learned.trim().chars().take(360).collect();
    if learned.is_empty() {
        return String::new();
    }
    format!("What you have learned about them. Use it. Do not recite it.\n{learned}")
}

/// Base rules, plus a short learned brief when the cabin already knows them.
/// Empty brief stays byte-identical to `CABIN_DESKTOP_RULES`.
pub fn cabin_rules(learned: &str) -> String {
    let tail = cabin_learned_tail(learned);
    if tail.is_empty() {
        return (*CABIN_DESKTOP_RULES).to_string();
    }
    format!("{}\n{tail}", *CABIN_DESKTOP_RULES)
}

/// One line on the cabin rules while Settings → desktop control is on.
/// Off stays byte-identical to [`cabin_rules`].
pub const DESKTOP_CABIN_LINE: &str =
    "Prefer the grokhub-desktop tools over shell xdotool or PowerShell for the screen. \
     Take a screenshot only when the task needs to see the screen, never for a greeting or plain chat. \
     If a screenshot fails, do not try it again; say what blocked it.";

pub fn cabin_rules_for(learned: &str, desktop: bool) -> String {
    let base = cabin_rules(learned);
    if !desktop {
        return base;
    }
    // Desktop line stays on the static rules, after the learned brief for the
    // combined helper. Prompt compose splits static vs learned separately.
    format!("{base}\n{DESKTOP_CABIN_LINE}")
}

/// Where the cabin runs, named for the OS it was built for, so the model never
/// guesses Linux on a Windows PC. Fixed at build time: the cached prompt prefix
/// stays byte-stable on each machine.
pub const HOST_PLACE: &str = if cfg!(windows) {
    "this Windows PC"
} else if cfg!(target_os = "macos") {
    "this Mac"
} else {
    "this Linux desktop"
};

/// The OS and the shell `run_terminal_command` really uses, one sentence.
pub const HOST_SHELL_LINE: &str = if cfg!(windows) {
    "It runs Windows, and the shell tool is Windows PowerShell, so use PowerShell commands and Windows paths, never bash or WSL."
} else if cfg!(target_os = "macos") {
    "It runs macOS, and the shell tool is bash."
} else {
    "It runs Linux, and the shell tool is bash."
};

/// Headless GrokHub chat is the cabin assistant on this computer, not grok.com.
/// How a chat turn looks things up. Chat prompts only: background jobs carry
/// their playbook's steps instead, and stay lean.
pub const LOOKUP_RULES: &str = "Look things up with web_search, then web_fetch the pages you rely on; the browser is for acting on a page. Call tools directly, never through the shell. Search a game question (quests, items, bosses, lore, patches) or anything that changes over time before answering, unasked. Link only pages a tool opened this turn. Never offer to look something up unless a search worked this turn; if none did, say so in one line and mark what is from memory.";

/// The native engine puts these first in its system prompt.
pub static CABIN_DESKTOP_RULES: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(
    || {
        let rules: &'static str = Box::leak(format!("You are the cabin assistant on {HOST_PLACE} through {{product}}. {HOST_SHELL_LINE} You can do what this computer can do: files, shell, browser, and the desktop. Never say you lack access to this computer, files, or desktop. Do the next step with tools. Ask only before sending a message, paying, deleting something they did not name, or publishing. Never work around the sandbox, access denied or the admin prompt: ask in one line first, and never loosen a system protection (System Restore, Defender, firewall) to get past it. Keep helper files out of AppData. Never say you are doing something with their approval until they said yes or accepted the approval card; until then, say it needs their OK. Be warm, and brief when working. Do not repeat the chat. When they are just chatting or hanging out, stay in it with them and enjoy it: no task offers, no next steps, and no sign-offs like \"your call\", \"ping me whenever\" or \"want me to queue something?\". An obvious joke between friends, like hacking a buddy's PC to make them stop talking, gets a playful comeback in your own voice, never a lecture about laws or policy. When a request would really hurt someone, steer away in character, briefly and without a lecture. Talk with them like an adult friend would: most subjects are fine, mature ones included (drinking, dating, dark humor, frank questions), and you can match their swearing; no moralizing, no disclaimers, no \"as an AI\" hedges. Drop the jokes and be sincere about grief, health or real distress. Do not paste code, diffs, or logs unless they asked to see it. Keep your planning and notes to yourself: what they read is plain sentences. When a tool, a page, or a first pass comes back empty or wrong, try one other path. Then say what blocked you and the next useful step. Do not end the turn on that first miss. Do not invent a source, a count, or a fact. A stable preference or routine is one line: USER_FACT: why they asked and what would help next time, not a copy of their sentence. Separate long work that should not hold up this chat (a full test run, a big search, a batch elsewhere) is one line per task: BACKGROUND_TASK: complete, self-contained instructions. The cabin runs it beside this chat on its own card and puts the result on that card, not in this chat; you hear what it found on your next turn. Do not wait for it. When they say they want to reach something in a game they play, add one line: GAME_GOAL: game | goal in a few words | count, finish, achievements, skill, rating or custom | target number. When they say where they are now on such a goal, add one line: GAME_PROGRESS: game | goal | number. Never mention these lines.").into_boxed_str());
        crate::brand::filled(rules)
    },
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_asks_before_admin_rights_and_never_works_around_the_sandbox() {
        let rules = *CABIN_DESKTOP_RULES;
        assert!(rules.contains(
            "Never work around the sandbox, access denied or the admin prompt: ask in one line first"
        ));
        assert!(rules.contains("Keep helper files out of AppData"));
    }

    #[test]
    fn the_rules_name_the_os_and_shell_this_build_runs_on() {
        let rules = *CABIN_DESKTOP_RULES;
        if cfg!(windows) {
            assert!(rules.starts_with("You are the cabin assistant on this Windows PC through "));
            assert!(rules.contains(
                "It runs Windows, and the shell tool is Windows PowerShell, so use PowerShell commands and Windows paths, never bash or WSL."
            ));
            assert!(!rules.contains("Linux"), "Windows rules say Linux: {rules}");
        } else if cfg!(target_os = "macos") {
            assert!(rules.starts_with("You are the cabin assistant on this Mac through "));
            assert!(!rules.contains("Linux"));
        } else {
            assert!(rules.starts_with("You are the cabin assistant on this Linux desktop through "));
            assert!(rules.contains("It runs Linux, and the shell tool is bash."));
            assert!(!rules.contains("Windows"));
        }
        assert!(!rules.contains("{product}"));
    }

    #[test]
    fn plain_chat_stays_present_without_task_offers_or_sign_offs() {
        assert!((*CABIN_DESKTOP_RULES).contains(
            "Be warm, and brief when working. Do not repeat the chat. When they are just chatting or \
             hanging out, stay in it with them and enjoy it: no task offers, no next steps, and no \
             sign-offs like \"your call\", \"ping me whenever\" or \"want me to queue something?\"."
        ));
        assert!(!(*CABIN_DESKTOP_RULES).contains("Be brief and warm."));
    }

    #[test]
    fn lookups_search_first_and_never_go_through_the_shell() {
        assert!(LOOKUP_RULES.contains("web_search, then web_fetch the pages you rely on"));
        assert!(LOOKUP_RULES.contains("Call tools directly, never through the shell."));
        assert!(
            LOOKUP_RULES.contains("Search a game question (quests, items, bosses, lore, patches)")
        );
        assert!(LOOKUP_RULES.contains("Never offer to look something up unless a search worked"));
    }

    #[test]
    fn the_rules_ask_for_game_goal_lines_in_the_objectives_wording() {
        assert!((*CABIN_DESKTOP_RULES).contains(crate::objectives::GOAL_MARKER_RULE));
    }

    #[test]
    fn cabin_rules_keep_the_desktop_assistant_and_learned_brief() {
        assert_eq!(cabin_rules(""), *CABIN_DESKTOP_RULES);
        assert_eq!(cabin_rules_for("", false), *CABIN_DESKTOP_RULES);
        assert_eq!(cabin_static_rules(false), *CABIN_DESKTOP_RULES);
        assert!(cabin_learned_tail("").is_empty());
        let armed = cabin_rules_for("", true);
        assert!(armed.starts_with(*CABIN_DESKTOP_RULES));
        assert!(armed.contains(DESKTOP_CABIN_LINE));
        assert_eq!(
            cabin_static_rules(true),
            format!("{}\n{DESKTOP_CABIN_LINE}", *CABIN_DESKTOP_RULES)
        );
        assert!(armed.ends_with(
            "\nPrefer the grokhub-desktop tools over shell xdotool or PowerShell for the screen. Take a \
             screenshot only when the task needs to see the screen, never for a greeting or plain chat. If a \
             screenshot fails, do not try it again; say what blocked it."
        ));
        assert_ne!(armed, *CABIN_DESKTOP_RULES);
        let with = cabin_rules("Around 21:00 they skip night.");
        assert_eq!(
            cabin_rules_for("Around 21:00 they skip night.", false),
            with
        );
        assert!(with.starts_with(*CABIN_DESKTOP_RULES));
        assert!(with.contains("skip night"));
        assert!(with.contains("Do not recite"));
        let tail = cabin_learned_tail("Around 21:00 they skip night.");
        assert!(tail.contains("skip night") && !tail.contains("WORK_PIN"));
        assert!(
            (*CABIN_DESKTOP_RULES).contains("this computer")
                && (*CABIN_DESKTOP_RULES).contains("USER_FACT:")
                && (*CABIN_DESKTOP_RULES).contains("BACKGROUND_TASK:")
                && (*CABIN_DESKTOP_RULES)
                    .contains("puts the result on that card, not in this chat")
                && !(*CABIN_DESKTOP_RULES).contains("posts the result here")
                && (*CABIN_DESKTOP_RULES).contains("or publishing")
                && (*CABIN_DESKTOP_RULES).contains("try one other path")
                && (*CABIN_DESKTOP_RULES).contains("Do not invent a source"),
            "desktop rules must stay a proactive assistant: {}",
            *CABIN_DESKTOP_RULES
        );
    }

    /// KB-04: nothing about the Workboard rides the system prompt. Board reads
    /// and writes are local; the model sees one resume note, in the resume
    /// turn's input only. Old `WORK_PIN:` / `WORK_UPDATE:` replies still parse.
    #[test]
    fn the_rules_say_nothing_about_the_workboard() {
        for rules in [
            cabin_static_rules(false),
            cabin_static_rules(true),
            cabin_rules_for("Around 21:00 they skip night.", true),
        ] {
            for word in [
                "WORK_PIN",
                "WORK_UPDATE",
                "workboard",
                "Workboard",
                "checkpoint",
                "work_",
            ] {
                assert!(!rules.contains(word), "{word} in the rules: {rules}");
            }
        }
        assert!(crate::workboard::parse_work_pin("WORK_PIN: Flash the pi | write image").is_some());
        assert!(
            crate::workboard::parse_work_update("WORK_UPDATE: Flash the pi | status=done")
                .is_some()
        );
    }

    #[test]
    fn approval_is_never_claimed_early_and_planning_stays_private() {
        assert!((*CABIN_DESKTOP_RULES).contains(
            "Never say you are doing something with their approval until they said yes or accepted \
             the approval card; until then, say it needs their OK."
        ));
        assert!((*CABIN_DESKTOP_RULES).contains(
            "Keep your planning and notes to yourself: what they read is plain sentences."
        ));
    }

    #[test]
    fn a_harmless_joke_gets_a_comeback_and_real_harm_is_steered_in_character() {
        assert!((*CABIN_DESKTOP_RULES).contains(
            "An obvious joke between friends, like hacking a buddy's PC to make them stop talking, \
             gets a playful comeback in your own voice, never a lecture about laws or policy. When a \
             request would really hurt someone, steer away in character, briefly and without a lecture."
        ));
    }

    #[test]
    fn every_voice_talks_to_an_adult_like_a_friend_and_turns_sincere_on_distress() {
        assert!((*CABIN_DESKTOP_RULES).contains(
            "Talk with them like an adult friend would: most subjects are fine, mature ones included \
             (drinking, dating, dark humor, frank questions), and you can match their swearing; no \
             moralizing, no disclaimers, no \"as an AI\" hedges. Drop the jokes and be sincere about \
             grief, health or real distress."
        ));
        // Openness never names sex chat: that stays behind the adults-only switch.
        assert!(!(*CABIN_DESKTOP_RULES).to_ascii_lowercase().contains("sex"));
    }
}
