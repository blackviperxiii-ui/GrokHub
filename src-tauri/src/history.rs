//! Chat history: the shell's past chats, read back from the session files the
//! engine already writes (`{config}/sessions/<id>.jsonl`, one per chat). The
//! History page lists, searches, renames and deletes them; opening one shows
//! its transcript, and the next message resumes it (`agent::serve`).
//!
//! Only the shell's own chats (ids starting `shell-`) show. The cabin keeps
//! its threads on its own History page, so no file has two writers.

use std::path::Path;
use std::time::UNIX_EPOCH;

use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{InputItem, SessionInfo};
use serde::Serialize;

/// The shell's conversation ids (`newConversationId` in ui/src/Chat.tsx).
pub const PREFIX: &str = "shell-";
/// A search lists at most this many chats.
pub const SEARCH_CAP: usize = 200;
/// A reopened chat shows its newest lines up to this many; Show earlier
/// asks for more.
pub const LOAD_CAP: usize = 200;

pub const NOT_SHELL: &str = "That chat isn't one of this app's chats.";
pub const NO_NAME: &str = "Give the chat a name.";

/// One row on the History page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRow {
    pub id: String,
    pub title: String,
    /// Unix ms the chat started.
    pub created: u64,
    /// Unix ms of its last saved turn; the page groups and sorts by it.
    pub updated: u64,
    /// Pinned with `/pin`: it lists first.
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    /// Tool steps are not replayed as cards; one muted line names them.
    Tools,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub role: Role,
    pub text: String,
}

/// A saved chat as the Chat page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: String,
    pub title: String,
    /// The newest lines, oldest first.
    pub lines: Vec<Line>,
    /// How many older lines were left out.
    pub earlier: usize,
}

fn shell_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.starts_with(PREFIX) {
        Ok(id)
    } else {
        Err(NOT_SHELL.into())
    }
}

/// When the chat's file last changed, or its start time if that can't be read.
/// Never before the start: the file system's coarse clock can stamp a file
/// written in the same millisecond a few ms behind `now_ms`.
fn updated_ms(info: &SessionInfo) -> u64 {
    grokhub_agent::session_file(&info.id)
        .ok()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(info.created, |at| (at.as_millis() as u64).max(info.created))
}

fn row(info: &SessionInfo) -> ChatRow {
    ChatRow {
        id: info.id.clone(),
        title: info.title.clone(),
        created: info.created,
        updated: updated_ms(info),
        pinned: false,
    }
}

const PINS_FILE: &str = "shell-pins.json";

/// The ids `/pin` has pinned, from `{config}/shell-pins.json`.
pub fn pinned_ids(config_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(config_dir.join(PINS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// `/pin`: pins the chat, or unpins it when it is pinned. True when it is
/// pinned now. A chat with no file yet can't be pinned.
pub fn toggle_pin(config_dir: &Path, id: &str) -> Result<bool, String> {
    let id = shell_id(id)?;
    {
        let _config = ConfigGuard::set(config_dir);
        grokhub_agent::load_session(id)?;
    }
    let mut ids = pinned_ids(config_dir);
    let pinned = if let Some(at) = ids.iter().position(|p| p == id) {
        ids.remove(at);
        false
    } else {
        ids.push(id.to_string());
        true
    };
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{PINS_FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, config_dir.join(PINS_FILE)).map_err(|e| e.to_string())?;
    Ok(pinned)
}

/// The shell's sessions, newest activity first. Call under a `ConfigGuard`.
fn shell_sessions() -> Vec<(ChatRow, SessionInfo)> {
    let mut found: Vec<(ChatRow, SessionInfo)> = grokhub_agent::list_sessions()
        .into_iter()
        .filter(|info| info.id.starts_with(PREFIX))
        .map(|info| (row(&info), info))
        .collect();
    let pins = pinned_ids(&grokhub_agent::perm::config_dir());
    for (row, _) in &mut found {
        row.pinned = pins.contains(&row.id);
    }
    found.sort_by(|(a, _), (b, _)| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.updated.cmp(&a.updated))
            .then(a.id.cmp(&b.id))
    });
    found
}

pub fn list(config_dir: &Path) -> Vec<ChatRow> {
    let _config = ConfigGuard::set(config_dir);
    shell_sessions().into_iter().map(|(row, _)| row).collect()
}

/// Chats whose title or transcript holds `query`, ignoring case. A blank
/// query lists them all.
pub fn search(config_dir: &Path, query: &str) -> Vec<ChatRow> {
    let needle = query.trim().to_lowercase();
    let _config = ConfigGuard::set(config_dir);
    shell_sessions()
        .into_iter()
        .filter(|(row, info)| {
            needle.is_empty()
                || row.title.to_lowercase().contains(&needle)
                || grokhub_agent::transcript_pairs(info)
                    .iter()
                    .any(|(_, text)| text.to_lowercase().contains(&needle))
        })
        .take(SEARCH_CAP)
        .map(|(row, _)| row)
        .collect()
}

/// The transcript's lines: user and assistant messages as they were shown,
/// and each run of tool steps folded into one `Tools` line naming them.
pub fn lines(info: &SessionInfo) -> Vec<Line> {
    // `transcript_pairs` writes a tool call as "Tool {name}\n{arguments}" and
    // its output as "Tool result\n{output}"; the names come from the input.
    let tools: Vec<String> = info
        .input()
        .into_iter()
        .filter_map(|item| match item {
            InputItem::FunctionCall { name, .. } => Some(name),
            _ => None,
        })
        .collect();
    let mut out: Vec<Line> = Vec::new();
    for (role, text) in grokhub_agent::transcript_pairs(info) {
        let assistant = role == "assistant";
        let call = tools
            .iter()
            .find(|name| assistant && text.starts_with(&format!("Tool {name}\n")));
        let result = assistant && text.starts_with("Tool result\n");
        match (call, result) {
            (Some(name), _) => match out.last_mut() {
                Some(Line {
                    role: Role::Tools,
                    text,
                }) => {
                    text.push_str(", ");
                    text.push_str(name);
                }
                _ => out.push(Line {
                    role: Role::Tools,
                    text: name.clone(),
                }),
            },
            (None, true) => {}
            (None, false) if assistant => out.push(Line {
                role: Role::Assistant,
                // The reply's chips block was never shown.
                text: grokhub_core::objectives::without_markers(
                    &grokhub_core::split_chip_trailer(&text).0,
                ),
            }),
            (None, false) => out.push(Line {
                role: Role::User,
                text,
            }),
        }
    }
    out
}

/// The chat's newest `limit` lines (at least one), oldest first.
pub fn load(config_dir: &Path, id: &str, limit: usize) -> Result<Transcript, String> {
    let id = shell_id(id)?;
    let _config = ConfigGuard::set(config_dir);
    let info = grokhub_agent::load_session(id)?;
    let mut all = lines(&info);
    let earlier = all.len().saturating_sub(limit.max(1));
    let lines = all.split_off(earlier);
    Ok(Transcript {
        id: info.id,
        title: info.title,
        lines,
        earlier,
    })
}

/// Renames a saved chat. A chat with no file yet can't be renamed.
pub fn rename(config_dir: &Path, id: &str, title: &str) -> Result<(), String> {
    let id = shell_id(id)?;
    if title.trim().is_empty() {
        return Err(NO_NAME.into());
    }
    let _config = ConfigGuard::set(config_dir);
    grokhub_agent::load_session(id)?;
    grokhub_agent::rename_session(id, title)
}

/// Deletes the chat's file and its files; a turn still running is not
/// written back. `/bg` and scheduled forks of it go with it.
pub fn delete(config_dir: &Path, id: &str) -> Result<(), String> {
    let id = shell_id(id)?;
    let _config = ConfigGuard::set(config_dir);
    grokhub_agent::delete_session(id)?;
    grokhub_agent::delete_forks_of(&[id.to_string()]);
    let mut ids = pinned_ids(config_dir);
    if let Some(at) = ids.iter().position(|p| p == id) {
        ids.remove(at);
        if let Ok(raw) = serde_json::to_string(&ids) {
            let _ = std::fs::write(config_dir.join(PINS_FILE), raw);
        }
    }
    Ok(())
}

/// Takes one message out of a saved shell chat, so the model never reads it
/// again: the ask `text` (`later` counts the same ask sent after it), or with
/// `reply` the reply to that ask (no `text`: the reply before any ask).
/// False when the saved chat had no copy of it. A running reply refuses.
pub fn forget(
    config_dir: &Path,
    id: &str,
    reply: bool,
    text: Option<&str>,
    later: usize,
) -> Result<bool, String> {
    let id = shell_id(id)?;
    let _config = ConfigGuard::set(config_dir);
    let what = match (reply, text) {
        (true, ask) => grokhub_agent::Forget::Reply {
            ask: ask.map(|text| (text, later)),
        },
        (false, Some(text)) => grokhub_agent::Forget::Ask { text, later },
        (false, None) => return Err(NO_MESSAGE.into()),
    };
    Ok(grokhub_agent::forget_message(id, what)? == grokhub_agent::Forgotten::Removed)
}

/// Delete message… with no message named.
pub const NO_MESSAGE: &str = "Pick the message to delete.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Route, Session, SessionMode};
    use grokhub_agent::{
        AuthKind, CancelToken, ClientError, ContentPart, DesktopOps, FunctionCall, ModelClient,
        PermMode, ResponsesRequest, StreamEvent, ToolOutput, TurnOutput,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-history-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    struct NoDesktop;

    impl DesktopOps for NoDesktop {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, _name: &str, _args: &serde_json::Value) -> ToolOutput {
            ToolOutput::err("no desktop in tests")
        }
    }

    fn no_desktop() -> Box<dyn DesktopOps> {
        Box::new(NoDesktop)
    }

    /// Replies with `reply`, after one `read_file` call when `tool` is set,
    /// and keeps every request it got.
    struct Model {
        reply: &'static str,
        tool: bool,
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl Model {
        fn new(reply: &'static str, tool: bool) -> Arc<Self> {
            Arc::new(Self {
                reply,
                tool,
                requests: Mutex::new(Vec::new()),
            })
        }

        /// The (role, text) of every message in the request at `index`.
        fn messages(&self, index: usize) -> Vec<(String, String)> {
            let requests = self.requests.lock().expect("requests");
            requests[index]
                .input
                .iter()
                .filter_map(|item| match item {
                    InputItem::Message { role, content } if role != "system" => Some((
                        role.clone(),
                        content
                            .iter()
                            .filter_map(|part| match part {
                                ContentPart::InputText(text) => Some(text.as_str()),
                                ContentPart::InputImage(_) => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )),
                    _ => None,
                })
                .collect()
        }
    }

    impl ModelClient for Model {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut requests = self.requests.lock().expect("requests");
            requests.push(req.clone());
            let asked_tool = req
                .input
                .iter()
                .any(|item| matches!(item, InputItem::FunctionCallOutput { .. }));
            if self.tool && !asked_tool {
                return Ok(TurnOutput {
                    calls: vec![FunctionCall {
                        call_id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: r#"{"target_file":"a.txt"}"#.into(),
                    }],
                    ..TurnOutput::default()
                });
            }
            sink(StreamEvent::TextDelta(self.reply.into()));
            Ok(TurnOutput {
                text: self.reply.into(),
                ..TurnOutput::default()
            })
        }
    }

    /// One message through a fresh worker for `id`, as the shell runs it.
    fn turn(id: &str, config: &Path, workspace: &Path, model: Arc<Model>, text: &str) {
        let session = Session::spawn_with(id, config.to_path_buf(), no_desktop);
        let route = Route {
            client: model,
            auth_kind: AuthKind::ApiKey,
            workspace: workspace.to_path_buf(),
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Chat,
            voice: String::new(),
            voice_session: None,
            suggestions: true,
        };
        assert_eq!(
            session.prompt(text, Vec::new(), route, Box::new(|_| {})),
            Ok("end_turn".to_string())
        );
    }

    fn line(role: Role, text: &str) -> Line {
        Line {
            role,
            text: text.into(),
        }
    }

    #[test]
    fn a_chat_lists_loads_and_resumes_with_its_first_turn() {
        let (config, workspace) = (temp_dir("resume-cfg"), temp_dir("resume-ws"));
        let id = "shell-history-resume";
        let first = Model::new("Hello back.", false);
        turn(id, &config, &workspace, Arc::clone(&first), "hello there");

        let rows = list(&config);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0].id.as_str(), rows[0].title.as_str()),
            (id, "hello there")
        );
        assert!(rows[0].updated >= rows[0].created, "{rows:?}");

        let shown = load(&config, id, LOAD_CAP).expect("load");
        assert_eq!(
            shown,
            Transcript {
                id: id.into(),
                title: "hello there".into(),
                lines: vec![
                    line(Role::User, "hello there"),
                    line(Role::Assistant, "Hello back.")
                ],
                earlier: 0,
            }
        );

        // The worker for that chat is gone (New chat, a restart); a new one
        // with the same id sends the first turn along with the next message.
        let second = Model::new("Still here.", false);
        turn(
            id,
            &config,
            &workspace,
            Arc::clone(&second),
            "are you there?",
        );
        let sent = second.messages(0);
        let texts: Vec<&str> = sent.iter().map(|(_, text)| text.as_str()).collect();
        let at = |needle: &str| texts.iter().position(|text| text.ends_with(needle));
        let (hello, back, again) = (
            at("hello there").expect("first question resent"),
            at("Hello back.").expect("first reply resent"),
            at("are you there?").expect("new question"),
        );
        assert!(hello < back && back < again, "{sent:?}");
        assert_eq!(sent[back].0, "assistant");

        let shown = load(&config, id, LOAD_CAP).expect("load");
        assert_eq!(
            shown.lines,
            vec![
                line(Role::User, "hello there"),
                line(Role::Assistant, "Hello back."),
                line(Role::User, "are you there?"),
                line(Role::Assistant, "Still here."),
            ]
        );
        // Only the newest lines come back past the limit.
        let tail = load(&config, id, 2).expect("tail");
        assert_eq!(
            (tail.earlier, tail.lines),
            (
                2,
                vec![
                    line(Role::User, "are you there?"),
                    line(Role::Assistant, "Still here.")
                ]
            )
        );
        assert_eq!(list(&config).len(), 1);
    }

    #[test]
    fn a_replys_chips_block_becomes_follow_ups_and_never_shows() {
        let (config, workspace) = (temp_dir("chips-cfg"), temp_dir("chips-ws"));
        let id = "shell-history-chips";
        let model = Model::new(
            "Plug it in, then pick it in Settings.\n<chips>\nask|Explain the second step in more detail|Explain step two\nopen|automations|See automations\n</chips>",
            false,
        );
        let session = Session::spawn_with(id, config.clone(), no_desktop);
        let route = Route {
            client: Arc::clone(&model) as Arc<dyn ModelClient + Send + Sync>,
            auth_kind: AuthKind::ApiKey,
            workspace: workspace.clone(),
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Plan,
            voice: String::new(),
            voice_session: None,
            suggestions: true,
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        assert_eq!(
            session.prompt(
                "how do I set up the backup drive?",
                Vec::new(),
                route,
                Box::new(move |event| sink.lock().expect("events").push(event)),
            ),
            Ok("end_turn".to_string())
        );
        // The chips rule rode on the cached rules.
        let asked = model.requests.lock().expect("requests")[0].clone();
        assert!(
            asked.input.iter().any(|item| matches!(
                item,
                InputItem::Message { role, content } if role == "system"
                    && content.iter().any(|part| matches!(
                        part,
                        ContentPart::InputText(text) if text.contains(grokhub_core::CHIP_TRAILER_RULE)
                    ))
            )),
            "no chips rule in the system prompt"
        );
        let events = events.lock().expect("events").clone();
        let shown: String = events
            .iter()
            .filter_map(|event| match event {
                crate::agent::UiEvent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(shown, "Plug it in, then pick it in Settings.\n");
        let ups = events
            .iter()
            .find_map(|event| match event {
                crate::agent::UiEvent::Suggestions { items } => Some(items.clone()),
                _ => None,
            })
            .expect("follow-ups");
        assert_eq!(
            ups.iter()
                .map(|c| (c.label.as_str(), c.view, c.mode.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Explain step two", None, "plan"),
                ("See automations", Some("automations"), ""),
            ]
        );
        // The saved chat shows the reply as it was shown, and its follow-ups
        // stay with it until the chat moves on.
        let lines = load(&config, id, LOAD_CAP).expect("load").lines;
        assert_eq!(
            lines.last(),
            Some(&line(
                Role::Assistant,
                "Plug it in, then pick it in Settings.\n"
            ))
        );
        let saved = crate::chips::follow_ups(&config, id, 1).expect("saved");
        assert_eq!((saved.chips.len(), saved.mode.as_str()), (2, "plan"));
        assert_eq!(crate::chips::follow_ups(&config, id, 2), None);
    }

    #[test]
    fn a_saved_chat_exports_as_markdown_and_never_over_an_older_file() {
        let (config, workspace) = (temp_dir("export-cfg"), temp_dir("export-ws"));
        let out = temp_dir("export-out");
        let id = "shell-history-export";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Blue suits it.", true),
            "pick a colour",
        );
        let first = crate::export::export_into(&config, id, &out, 0, 0).expect("export");
        let again = crate::export::export_into(&config, id, &out, 0, 0).expect("export again");
        assert_eq!(
            first.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000.md")
        );
        assert_eq!(
            again.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000-2.md")
        );
        let md = std::fs::read_to_string(&first).expect("read");
        for part in [
            "# pick a colour",
            "pick a colour",
            "Used read_file",
            "Blue suits it.",
        ] {
            assert!(md.contains(part), "{part} missing from {md}");
        }
    }

    #[test]
    fn tool_steps_fold_into_one_muted_line() {
        let (config, workspace) = (temp_dir("tools-cfg"), temp_dir("tools-ws"));
        std::fs::write(workspace.join("a.txt"), "alpha").expect("seed");
        let id = "shell-history-tools";
        turn(
            id,
            &config,
            &workspace,
            Model::new("It says alpha.", true),
            "read a.txt",
        );
        assert_eq!(
            load(&config, id, LOAD_CAP).expect("load").lines,
            vec![
                line(Role::User, "read a.txt"),
                line(Role::Tools, "read_file"),
                line(Role::Assistant, "It says alpha."),
            ]
        );
    }

    #[test]
    fn search_matches_titles_and_words_in_the_chat() {
        let (config, workspace) = (temp_dir("search-cfg"), temp_dir("search-ws"));
        turn(
            "shell-history-s1",
            &config,
            &workspace,
            Model::new("Try the BLUE one.", false),
            "pick a paint colour",
        );
        turn(
            "shell-history-s2",
            &config,
            &workspace,
            Model::new("Sure.", false),
            "plan a trip",
        );
        let ids = |rows: Vec<ChatRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(search(&config, "  Blue ")), vec!["shell-history-s1"]);
        assert_eq!(ids(search(&config, "TRIP")), vec!["shell-history-s2"]);
        assert_eq!(ids(search(&config, "kayak")), Vec::<String>::new());
        assert_eq!(search(&config, " ").len(), 2);
    }

    #[test]
    fn forgetting_a_message_keeps_it_from_the_next_turn() {
        let (config, workspace) = (temp_dir("forget-cfg"), temp_dir("forget-ws"));
        let id = "shell-history-forget";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Hi.", false),
            "keep this",
        );
        turn(
            id,
            &config,
            &workspace,
            Model::new("Noted the fruit.", false),
            "the code is kumquat",
        );
        assert_eq!(forget(&config, id, false, None, 0), Err(NO_MESSAGE.into()));
        assert_eq!(
            forget(&config, "native-cabin-1", false, Some("x"), 0),
            Err(NOT_SHELL.into())
        );
        assert_eq!(
            forget(&config, id, false, Some("the code is kumquat"), 0),
            Ok(true)
        );
        // The reply to "keep this" goes whole; the later reply now follows it.
        assert_eq!(forget(&config, id, true, Some("keep this"), 0), Ok(true));
        assert_eq!(forget(&config, id, false, Some("never said"), 0), Ok(false));
        let shown = load(&config, id, LOAD_CAP).expect("load");
        assert_eq!(shown.lines, vec![line(Role::User, "keep this")]);

        let next = Model::new("Ok.", false);
        turn(id, &config, &workspace, Arc::clone(&next), "what now");
        let sent = format!("{:?}", next.messages(0));
        assert!(
            !sent.contains("kumquat") && !sent.contains("fruit"),
            "{sent}"
        );
        assert!(sent.contains("keep this"), "{sent}");
    }

    #[test]
    fn rename_and_delete_change_only_the_shell_chat() {
        let (config, workspace) = (temp_dir("edit-cfg"), temp_dir("edit-ws"));
        let id = "shell-history-edit";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Ok.", false),
            "first draft",
        );
        {
            // A cabin chat in the same folder stays off this page.
            let _config = ConfigGuard::set(&config);
            grokhub_agent::rename_session("native-cabin-1", "Cabin chat").expect("cabin file");
        }
        assert_eq!(
            list(&config).into_iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![id]
        );
        assert_eq!(
            load(&config, "native-cabin-1", LOAD_CAP),
            Err(NOT_SHELL.into())
        );
        assert_eq!(delete(&config, "native-cabin-1"), Err(NOT_SHELL.into()));
        assert!(config
            .join("sessions")
            .join("native-cabin-1.jsonl")
            .exists());

        assert_eq!(rename(&config, id, "  "), Err(NO_NAME.into()));
        rename(&config, id, "Final plan").expect("rename");
        assert_eq!(list(&config)[0].title, "Final plan");
        assert!(rename(&config, "shell-history-none", "Ghost").is_err());
        assert!(!config
            .join("sessions")
            .join("shell-history-none.jsonl")
            .exists());

        let file = config.join("sessions").join(format!("{id}.jsonl"));
        assert!(file.exists());
        let fork = {
            let _config = ConfigGuard::set(&config);
            grokhub_agent::fork_session(id).expect("fork")
        };
        let fork_file = config.join("sessions").join(format!("{}.jsonl", fork.id));
        assert!(fork_file.exists());
        delete(&config, id).expect("delete");
        assert!(!file.exists());
        assert!(
            !fork_file.exists(),
            "a /bg fork holds a copy of the chat, so it goes too"
        );
        assert_eq!(list(&config), Vec::<ChatRow>::new());
        assert!(load(&config, id, LOAD_CAP).is_err());
    }

    fn applied_text(applied: crate::slash_ops::Applied) -> String {
        match applied.effect {
            crate::slash::Effect::Notice { text } => text,
            other => panic!("expected a notice, got {other:?}"),
        }
    }

    fn ctx<'a>(config: &'a Path, id: &'a str) -> crate::slash_ops::Ctx<'a> {
        crate::slash_ops::Ctx {
            config_dir: config,
            conversation_id: Some(id),
            bound: "",
            owned: None,
        }
    }

    #[test]
    fn rename_and_pin_act_on_the_chat_and_a_pinned_chat_lists_first() {
        use crate::slash::Op;
        use crate::slash_ops::run;
        let (config, workspace) = (temp_dir("pin-cfg"), temp_dir("pin-ws"));
        let (older, newer) = ("shell-history-pin-a", "shell-history-pin-b");
        turn(
            older,
            &config,
            &workspace,
            Model::new("Ok.", false),
            "first",
        );
        turn(
            newer,
            &config,
            &workspace,
            Model::new("Ok.", false),
            "second",
        );
        let text = applied_text(
            run(Op::Rename("Pier light".into()), &ctx(&config, older)).expect("rename"),
        );
        assert_eq!(text, "Renamed to Pier light");
        assert_eq!(
            list(&config)
                .iter()
                .find(|r| r.id == older)
                .map(|r| r.title.as_str()),
            Some("Pier light")
        );
        assert!(list(&config).iter().all(|r| !r.pinned));
        let text = applied_text(run(Op::Pin, &ctx(&config, older)).expect("pin"));
        assert_eq!(text, "Pinned Pier light");
        let rows = list(&config);
        assert_eq!(rows[0].id, older);
        assert!(rows[0].pinned && !rows[1].pinned);
        assert_eq!(
            search(&config, "").first().map(|r| r.id.as_str()),
            Some(older)
        );
        let text = applied_text(run(Op::Pin, &ctx(&config, older)).expect("unpin"));
        assert_eq!(text, "Unpinned Pier light");
        assert!(list(&config).iter().all(|r| !r.pinned));
        // A chat that was never saved can't be renamed or pinned.
        assert_eq!(
            run(Op::Pin, &ctx(&config, "shell-history-pin-none")),
            Err("Send a message first. This chat isn't saved yet.".into())
        );
        assert_eq!(
            run(Op::Rename("x".into()), &ctx(&config, "native-cabin-1")),
            Err(NOT_SHELL.into())
        );
    }

    #[test]
    fn delete_asks_first_and_changes_nothing_until_yes() {
        use crate::slash::{Effect, Op};
        use crate::slash_ops::run;
        let (config, workspace) = (temp_dir("del-cfg"), temp_dir("del-ws"));
        let id = "shell-history-del";
        turn(id, &config, &workspace, Model::new("Ok.", false), "first");
        run(Op::Rename("Pier light".into()), &ctx(&config, id)).expect("rename");
        run(Op::Pin, &ctx(&config, id)).expect("pin");
        let asked = run(Op::Delete { yes: false }, &ctx(&config, id)).expect("asks");
        assert_eq!(
            asked.effect,
            Effect::Confirm {
                prompt: "Delete Pier light? The chat and its files are removed for good.".into(),
                yes: "/delete yes".into(),
            }
        );
        assert_eq!(list(&config).len(), 1, "no yes, nothing deleted");
        assert!(config.join("sessions").join(format!("{id}.jsonl")).exists());
        let done = run(Op::Delete { yes: true }, &ctx(&config, id)).expect("deleted");
        assert_eq!(
            done.effect,
            Effect::Deleted {
                id: id.into(),
                title: "Pier light".into(),
            }
        );
        assert_eq!(list(&config), Vec::<ChatRow>::new());
        assert_eq!(pinned_ids(&config), Vec::<String>::new());
    }

    #[test]
    fn context_and_usage_read_the_saved_chat_and_the_browser_sees_only_its_own() {
        use crate::slash::Op;
        use crate::slash_ops::{run, Ctx};
        let (config, workspace) = (temp_dir("ctx-cfg"), temp_dir("ctx-ws"));
        let id = "shell-history-ctx";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Blue suits it.", false),
            "pick a colour",
        );
        let text = applied_text(run(Op::Context, &ctx(&config, id)).expect("context"));
        assert!(text.starts_with("1 turn · "), "{text}");
        assert!(text.contains(" tokens · ") && text.ends_with('%'), "{text}");
        assert!(!applied_text(run(Op::Usage, &ctx(&config, id)).expect("usage")).is_empty());
        let mine = |c: &str| c == id;
        let browser = Ctx {
            config_dir: &config,
            conversation_id: Some(id),
            bound: "",
            owned: Some(&mine),
        };
        assert!(run(Op::Context, &browser).is_ok());
        let sessions = applied_text(run(Op::Sessions, &browser).expect("sessions"));
        assert_eq!(sessions, "pick a colour");
        let other = Ctx {
            conversation_id: Some("shell-someone-else"),
            ..browser
        };
        assert_eq!(
            run(Op::Context, &other),
            Err(crate::remote::NOT_YOURS.to_string())
        );
        // On this computer /sessions opens History.
        let here = run(Op::Sessions, &ctx(&config, id)).expect("sessions");
        assert_eq!(here.effect, crate::slash::Effect::Open { view: "history" });
    }

    #[test]
    fn rewind_files_restores_a_checkpointed_file_only_after_a_yes() {
        use crate::slash::{Effect, Op};
        use crate::slash_ops::run;
        let config = temp_dir("rw-cfg");
        let work = temp_dir("rw-ws");
        let id = "shell-history-rewind";
        let file = work.join("notes.txt");
        std::fs::write(&file, "before").expect("seed");
        {
            let _config = ConfigGuard::set(&config);
            grokhub_agent::begin_step(id).expect("step");
            grokhub_agent::before_write(&file);
        }
        std::fs::write(&file, "after").expect("edit");
        let list_text = applied_text(run(Op::RewindList, &ctx(&config, id)).expect("list"));
        assert!(list_text.contains("notes.txt"), "{list_text}");
        let asked = run(Op::RewindFiles { yes: false }, &ctx(&config, id)).expect("asks");
        let Effect::Confirm { prompt, yes } = asked.effect else {
            panic!("expected a question");
        };
        assert!(prompt.contains("notes.txt"), "{prompt}");
        assert_eq!(yes, "/rewind files yes");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "after");
        applied_text(run(Op::RewindFiles { yes: true }, &ctx(&config, id)).expect("restore"));
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "before");
        assert_eq!(
            run(Op::RewindStep(99), &ctx(&config, id)),
            Err("No checkpoint for step 99".to_string())
        );
        assert_eq!(
            applied_text(run(Op::RewindNone, &ctx(&config, id)).expect("none")),
            crate::slash_ops::REWIND_NA
        );
        assert_eq!(
            run(
                Op::RewindFiles { yes: true },
                &ctx(&config, "shell-history-no-steps")
            ),
            Err("No agent file checkpoints yet. Edit a file in this chat first.".to_string())
        );
    }

    #[test]
    fn a_saved_chat_exports_as_html_and_json() {
        let (config, workspace) = (temp_dir("fmt-cfg"), temp_dir("fmt-ws"));
        let out = temp_dir("fmt-out");
        let id = "shell-history-fmt";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Blue suits it.", false),
            "pick a colour",
        );
        let html =
            crate::export::export_into_as(&config, id, &out, 0, 0, grokhub_core::ChatExport::Html)
                .expect("html");
        assert_eq!(
            html.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000.html")
        );
        let page = std::fs::read_to_string(&html).expect("read");
        for part in ["pick a colour", "Blue suits it."] {
            assert!(page.contains(part), "{part} missing from {page}");
        }
        let json =
            crate::export::export_into_as(&config, id, &out, 0, 0, grokhub_core::ChatExport::Json)
                .expect("json");
        assert_eq!(
            json.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000.json")
        );
        let raw = std::fs::read_to_string(&json).expect("read");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        assert!(value.is_object(), "{raw}");
        assert!(
            raw.contains("Blue suits it.") && raw.contains("pick a colour"),
            "{raw}"
        );
    }
}
