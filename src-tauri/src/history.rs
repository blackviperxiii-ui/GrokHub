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
    pub model: String,
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
fn updated_ms(info: &SessionInfo) -> u64 {
    grokhub_agent::session_file(&info.id)
        .ok()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(info.created, |at| at.as_millis() as u64)
}

fn row(info: &SessionInfo) -> ChatRow {
    ChatRow {
        id: info.id.clone(),
        title: info.title.clone(),
        created: info.created,
        updated: updated_ms(info),
        model: info.model.clone(),
    }
}

/// The shell's sessions, newest activity first. Call under a `ConfigGuard`.
fn shell_sessions() -> Vec<(ChatRow, SessionInfo)> {
    let mut found: Vec<(ChatRow, SessionInfo)> = grokhub_agent::list_sessions()
        .into_iter()
        .filter(|info| info.id.starts_with(PREFIX))
        .map(|info| (row(&info), info))
        .collect();
    found.sort_by(|(a, _), (b, _)| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
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
            (None, false) => out.push(Line {
                role: if role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                },
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
            model: "grok-4.7".into(),
            workspace: workspace.to_path_buf(),
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Chat,
            voice: String::new(),
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
            (
                rows[0].id.as_str(),
                rows[0].title.as_str(),
                rows[0].model.as_str()
            ),
            (id, "hello there", "grok-4.7")
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
}
