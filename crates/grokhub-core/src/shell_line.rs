//! One shell line sent to the agent: the Command page and `/sh` or `$ ` in
//! Chat. The chat message asks the agent to run the line exactly once, and
//! the chat shows the line itself (`$ git status`), not the message.

const ASK: &str = "Run this shell command in the project folder, exactly as written and once, \
                   then show me what it printed and its exit code. Don't change it or run \
                   anything else.";

/// The chat message that asks the agent to run `command`. The fence is
/// longer than any run of backticks in the command, so it can't close early.
pub fn prompt(command: &str) -> String {
    let fence = "`".repeat((longest_backticks(command) + 1).max(3));
    format!("{ASK}\n\n{fence}sh\n{command}\n{fence}")
}

/// The command in a message [`prompt`] made, or `None` for any other text.
pub fn command_of(text: &str) -> Option<&str> {
    let body = text.strip_prefix(ASK)?.strip_prefix("\n\n")?;
    let ticks = body.len() - body.trim_start_matches('`').len();
    if ticks < 3 {
        return None;
    }
    let fence = &body[..ticks];
    let command = body[ticks..]
        .strip_prefix("sh\n")?
        .strip_suffix(fence)?
        .strip_suffix('\n')?;
    // The same fence [`prompt`] would pick, and one line, or it isn't ours.
    if command.contains(['\n', '\r']) || ticks != (longest_backticks(command) + 1).max(3) {
        return None;
    }
    Some(command)
}

fn longest_backticks(text: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_reads_back_as_the_line_typed() {
        assert_eq!(
            prompt("git status"),
            "Run this shell command in the project folder, exactly as written and once, \
             then show me what it printed and its exit code. Don't change it or run \
             anything else.\n\n```sh\ngit status\n```"
        );
        assert_eq!(command_of(&prompt("git status")), Some("git status"));
        assert_eq!(command_of(&prompt("echo ```x```")), Some("echo ```x```"));
        assert!(prompt("echo ```x```").contains("````sh\necho ```x```\n````"));
    }

    #[test]
    fn other_text_is_not_a_shell_line() {
        assert_eq!(command_of("hello"), None);
        assert_eq!(command_of(&(prompt("ls") + "\nmore")), None);
        assert_eq!(command_of(&prompt("ls").replace("```sh", "````sh")), None);
        assert_eq!(command_of(&format!("{ASK}\n\n```sh\nls\npwd\n```")), None);
        assert_eq!(command_of(&format!("{ASK}\n\n```bash\nls\n```")), None);
    }
}
