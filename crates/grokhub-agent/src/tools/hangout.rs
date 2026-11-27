//! Chat-only Hangout (Order 0.5): casual turns answer as chat.
//!
//! Tools are offered only when the person asks for an action (search, file,
//! command, or image) or the turn is a task / repair / research / background /
//! picture ask. An identical web_search / web_fetch / image_generate call in
//! the same session reuses the earlier result (see
//! [`crate::perm::earlier_result`]). A tool call on a chat-only turn gets one
//! plain [`CANT_DO_HERE`] and no retry.

use crate::brain::{is_picture_ask, is_video_ask, Brief, Intent};

/// Plain refusal when a chat-only Hangout turn cannot run a tool.
pub const CANT_DO_HERE: &str = "I can't do that here";

/// Verbs / phrases that ask for a web lookup.
const SEARCH_ASK: &[&str] = &[
    "search",
    "google",
    "look up",
    "lookup",
    "look it up",
    "find online",
    "search for",
    "search the web",
    "web search",
];

/// Verbs that ask for a file or folder action.
const FILE_ASK: &[&str] = &[
    "read",
    "open",
    "list",
    "write",
    "edit",
    "create",
    "delete",
    "rename",
    "save",
    "grep",
    "find file",
    "show file",
    "show the file",
];

/// Nouns that make a file ask concrete.
const FILE_NOUN: &[&str] = &[
    "file",
    "files",
    "folder",
    "folders",
    "directory",
    "dir",
    "path",
    "note",
    "notes",
    "readme",
    "code",
    "script",
];

/// Verbs that ask for a shell command.
const COMMAND_ASK: &[&str] = &[
    "run",
    "execute",
    "shell",
    "terminal",
    "command",
    "bash",
    "powershell",
];

/// True when the ask names an action that needs a tool: search, file, command,
/// or image.
pub fn asks_for_action(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    if is_picture_ask(&t) || is_video_ask(&t) {
        return true;
    }
    if SEARCH_ASK.iter().any(|p| t.contains(p)) {
        return true;
    }
    if COMMAND_ASK.iter().any(|p| {
        t.contains(p)
            && (t.contains("command")
                || t.contains("shell")
                || t.contains("terminal")
                || t.contains("bash")
                || t.contains("powershell")
                || t.contains("script"))
    }) {
        return true;
    }
    let has_file_verb = FILE_ASK.iter().any(|p| t.contains(p));
    let has_file_noun = FILE_NOUN.iter().any(|p| t.contains(p));
    has_file_verb && has_file_noun
}

/// True when this turn should send no tools: casual chat or creative play with
/// no action ask. A question keeps tools, since it may be about a file or an
/// earlier chat. So does a task, repair, research, background, or picture ask
/// (or any action ask), and an unclear ask: a `default` Brief is a guess, not
/// a read that the turn is small talk.
pub fn chat_only(brief: &Brief, text: &str) -> bool {
    if brief.by == "default" || asks_for_action(text) {
        return false;
    }
    match brief.intent {
        Intent::Chat | Intent::Creative => true,
        Intent::Question
        | Intent::Task
        | Intent::Repair
        | Intent::Research
        | Intent::Background
        | Intent::Image => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::Brief;

    fn brief(intent: Intent) -> Brief {
        Brief {
            intent,
            confidence: 100,
            ..Default::default()
        }
    }

    #[test]
    fn an_unclear_ask_keeps_its_tools() {
        let guess = Brief {
            intent: Intent::Chat,
            by: "default".into(),
            ..Default::default()
        };
        assert!(!chat_only(&guess, "do the thing"));
    }

    #[test]
    fn a_question_keeps_its_tools() {
        assert!(!chat_only(&brief(Intent::Question), "what does a.txt say?"));
        assert!(!chat_only(
            &brief(Intent::Question),
            "why is my fan so loud"
        ));
    }

    #[test]
    fn casual_chat_is_chat_only_and_a_search_ask_is_not() {
        assert!(chat_only(&brief(Intent::Chat), "hey how are you"));
        assert!(chat_only(&brief(Intent::Chat), "lol same"));
        assert!(chat_only(
            &brief(Intent::Chat),
            "how do I flirt with someone"
        ));
        assert!(!chat_only(
            &brief(Intent::Chat),
            "search the web for rust release notes"
        ));
        assert!(!chat_only(&brief(Intent::Task), "hey how are you"));
        assert!(!chat_only(&brief(Intent::Image), "draw me a fox"));
        assert!(!asks_for_action("hey how are you"));
        assert!(asks_for_action("search for the weather"));
        assert!(asks_for_action("read the notes file"));
        assert!(asks_for_action("run a shell command to list files"));
        assert!(asks_for_action("draw me a red fox"));
        assert!(!chat_only(
            &brief(Intent::Chat),
            "Nyx send me a picture of yourself"
        ));
        assert!(!chat_only(&brief(Intent::Chat), "send a selfie"));
        assert!(!chat_only(
            &brief(Intent::Chat),
            "make a short video of ocean waves"
        ));
    }
}
