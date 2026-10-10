//! The cabin's standing rules for the model: who it is on this computer and
//! the work-tracking lines it may emit.

/// Base rules, plus a short learned brief when the cabin already knows them.
/// Empty brief stays byte-identical to `CABIN_DESKTOP_RULES`.
pub fn cabin_rules(learned: &str) -> String {
    let learned: String = learned.trim().chars().take(360).collect();
    if learned.is_empty() {
        return CABIN_DESKTOP_RULES.to_string();
    }
    format!(
        "{CABIN_DESKTOP_RULES}\nWhat you have learned about them. Use it. Do not recite it.\n{learned}"
    )
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
    format!("{base}\n{DESKTOP_CABIN_LINE}")
}

/// Headless GrokHub chat is the cabin assistant on this Linux box, not grok.com.
/// The native engine puts these first in its system prompt.
pub const CABIN_DESKTOP_RULES: &str = "You are the cabin assistant on this Linux desktop through GrokHub. You can do what this computer can do: files, shell, browser, and the desktop. Never say you lack access to this computer, files, or desktop. Do the next step with tools. Ask only before sending a message, paying, deleting something they did not name, or publishing. Be brief and warm. Do not repeat the chat. Do not paste code, diffs, or logs unless they asked to see it. When they hand you work, track it with WORK_PIN and WORK_UPDATE and keep going. A paused workboard card is still yours. Resume it. When a tool, a page, or a first pass comes back empty or wrong, try one other path. Then say what blocked you and the next useful step. Do not end the turn on that first miss. Do not invent a source, a count, or a fact. A stable preference or routine is one line: USER_FACT: why they asked and what would help next time, not a copy of their sentence. Separate long work that should not hold up this chat (a full test run, a big search, a batch elsewhere) is one line per task: BACKGROUND_TASK: complete, self-contained instructions. The cabin runs it beside this chat and posts the result here; do not wait for it.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabin_rules_keep_the_desktop_assistant_and_learned_brief() {
        assert_eq!(cabin_rules(""), CABIN_DESKTOP_RULES);
        assert_eq!(cabin_rules_for("", false), CABIN_DESKTOP_RULES);
        let armed = cabin_rules_for("", true);
        assert!(armed.starts_with(CABIN_DESKTOP_RULES));
        assert!(armed.contains(DESKTOP_CABIN_LINE));
        assert!(armed.ends_with(
            "\nPrefer the grokhub-desktop tools over shell xdotool or PowerShell for the screen. Take a \
             screenshot only when the task needs to see the screen, never for a greeting or plain chat. If a \
             screenshot fails, do not try it again; say what blocked it."
        ));
        assert_ne!(armed, CABIN_DESKTOP_RULES);
        let with = cabin_rules("Around 21:00 they skip night.");
        assert_eq!(cabin_rules_for("Around 21:00 they skip night.", false), with);
        assert!(with.starts_with(CABIN_DESKTOP_RULES));
        assert!(with.contains("skip night"));
        assert!(with.contains("Do not recite"));
        assert!(
            CABIN_DESKTOP_RULES.contains("this computer")
                && CABIN_DESKTOP_RULES.contains("WORK_PIN")
                && CABIN_DESKTOP_RULES.contains("Resume it")
                && CABIN_DESKTOP_RULES.contains("USER_FACT:")
                && CABIN_DESKTOP_RULES.contains("BACKGROUND_TASK:")
                && CABIN_DESKTOP_RULES.contains("or publishing")
                && CABIN_DESKTOP_RULES.contains("try one other path")
                && CABIN_DESKTOP_RULES.contains("Do not invent a source"),
            "desktop rules must stay a proactive assistant: {CABIN_DESKTOP_RULES}"
        );
    }
}
