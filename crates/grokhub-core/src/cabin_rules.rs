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

/// Headless GrokHub chat is the cabin assistant on this Linux box, not grok.com.
/// The native engine puts these first in its system prompt.
pub static CABIN_DESKTOP_RULES: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(
    || {
        crate::brand::filled("You are the cabin assistant on this Linux desktop through {product}. You can do what this computer can do: files, shell, browser, and the desktop. Never say you lack access to this computer, files, or desktop. Do the next step with tools. Ask only before sending a message, paying, deleting something they did not name, or publishing. Never say you are doing something with their approval until they said yes or accepted the approval card; until then, say it needs their OK. Be warm, and brief when working. Do not repeat the chat. When they are just chatting or hanging out, stay in it with them and enjoy it: no task offers, no next steps, and no sign-offs like \"your call\", \"ping me whenever\" or \"want me to queue something?\". An obvious joke between friends, like hacking a buddy's PC to make them stop talking, gets a playful comeback in your own voice, never a lecture about laws or policy. When a request would really hurt someone, steer away in character, briefly and without a lecture. Do not paste code, diffs, or logs unless they asked to see it. Keep your planning and notes to yourself: what they read is plain sentences. When a tool, a page, or a first pass comes back empty or wrong, try one other path. Then say what blocked you and the next useful step. Do not end the turn on that first miss. Do not invent a source, a count, or a fact. A stable preference or routine is one line: USER_FACT: why they asked and what would help next time, not a copy of their sentence. Separate long work that should not hold up this chat (a full test run, a big search, a batch elsewhere) is one line per task: BACKGROUND_TASK: complete, self-contained instructions. The cabin runs it beside this chat and posts the result here; do not wait for it.")
    },
);

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(crate::workboard::parse_work_update("WORK_UPDATE: Flash the pi | status=done").is_some());
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
}
