//! Agent2Agent (A2A) v1.0 shapes for work one agent hands another: messages,
//! tasks, JSON-RPC calls and errors, agent cards, and an in-memory task book
//! that keeps the task lifecycle. Pure: no network and no files.
//! See `docs/design/agent-teams/README.md`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_team::Profile;

pub const PROTOCOL_VERSION: &str = "1.0";
/// The HTTP header a client names its A2A version in.
pub const VERSION_HEADER: &str = "A2A-Version";
pub const MAX_PART_BYTES: usize = 32 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_PARTS: usize = 16;
pub const MAX_ID: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

/// One part of a message. File parts parse, so they can be refused by name.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Data {
        data: Value,
        media_type: String,
    },
    File {
        filename: String,
        media_type: String,
    },
}

impl TryFrom<Value> for Part {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let Value::Object(map) = value else {
            return Err("a part is a JSON object".into());
        };
        let text_of = |key: &str| {
            map.get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        if let Some(text) = map.get("text") {
            let text = text.as_str().ok_or("a text part's text is a string")?;
            return Ok(Part::Text(text.to_string()));
        }
        if let Some(data) = map.get("data") {
            let media_type = match text_of("mediaType") {
                t if t.is_empty() => "application/json".to_string(),
                t => t,
            };
            return Ok(Part::Data {
                data: data.clone(),
                media_type,
            });
        }
        if map.contains_key("raw") || map.contains_key("url") {
            return Ok(Part::File {
                filename: text_of("filename"),
                media_type: text_of("mediaType"),
            });
        }
        Err("a part has text, data, raw or url".into())
    }
}

impl From<Part> for Value {
    fn from(part: Part) -> Value {
        match part {
            Part::Text(text) => json!({ "text": text }),
            Part::Data { data, media_type } => json!({ "data": data, "mediaType": media_type }),
            Part::File {
                filename,
                media_type,
            } => json!({ "url": "", "filename": filename, "mediaType": media_type }),
        }
    }
}

impl Serialize for Part {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Value::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Part {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Part::try_from(Value::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub message_id: String,
    #[serde(
        default,
        deserialize_with = "empty_as_unset",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "empty_as_unset",
        skip_serializing_if = "Option::is_none"
    )]
    pub task_id: Option<String>,
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
}

/// ProtoJSON reads an empty string field as unset.
fn empty_as_unset<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(de)?.filter(|id| !id.is_empty()))
}

impl Message {
    pub fn text(role: Role, message_id: &str, text: &str) -> Message {
        Message {
            message_id: message_id.to_string(),
            context_id: None,
            task_id: None,
            role,
            parts: vec![Part::Text(text.to_string())],
            metadata: Value::Null,
        }
    }

    /// What a message may carry between agents: text and JSON data only,
    /// within the size caps.
    pub fn check(&self) -> Result<(), A2aError> {
        if self.message_id.trim().is_empty() || self.message_id.len() > MAX_ID {
            return Err(A2aError::InvalidParams(format!(
                "messageId is 1 to {MAX_ID} characters"
            )));
        }
        for (name, id) in [("contextId", &self.context_id), ("taskId", &self.task_id)] {
            if id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty() || id.len() > MAX_ID)
            {
                return Err(A2aError::InvalidParams(format!(
                    "{name} is 1 to {MAX_ID} characters"
                )));
            }
        }
        if self.parts.is_empty() || self.parts.len() > MAX_PARTS {
            return Err(A2aError::InvalidParams(format!(
                "a message has 1 to {MAX_PARTS} parts"
            )));
        }
        let mut total = 0;
        for part in &self.parts {
            let size = match part {
                Part::Text(text) => text.len(),
                Part::Data { data, media_type } => {
                    if media_type != "application/json" {
                        return Err(A2aError::ContentTypeNotSupported(media_type.clone()));
                    }
                    data.to_string().len()
                }
                Part::File { media_type, .. } => {
                    let kind = if media_type.is_empty() {
                        "file"
                    } else {
                        media_type
                    };
                    return Err(A2aError::ContentTypeNotSupported(kind.to_string()));
                }
            };
            if size > MAX_PART_BYTES {
                return Err(A2aError::InvalidParams(format!(
                    "a part is at most {MAX_PART_BYTES} bytes"
                )));
            }
            total += size;
        }
        if total > MAX_MESSAGE_BYTES {
            return Err(A2aError::InvalidParams(format!(
                "a message is at most {MAX_MESSAGE_BYTES} bytes"
            )));
        }
        Ok(())
    }

    /// The message as prompt text: text parts as they are, data parts as JSON.
    pub fn plain_text(&self) -> String {
        let pieces: Vec<String> = self
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.clone()),
                Part::Data { data, .. } => Some(data.to_string()),
                Part::File { .. } => None,
            })
            .collect();
        pieces.join("\n\n")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
}

impl TaskState {
    /// Takes no more messages and can't be canceled.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
        )
    }

    /// Waiting on the sender; a follow-up message with the task's id goes on.
    pub fn is_interrupted(self) -> bool {
        matches!(self, TaskState::InputRequired | TaskState::AuthRequired)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Message>,
}

impl Task {
    /// The reply's text, for a completed task.
    pub fn reply(&self) -> Option<String> {
        let artifact = self.artifacts.first()?;
        Some(
            artifact
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    }
}

/// JSON-RPC and A2A errors, with their spec codes and names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A2aError {
    Parse,
    InvalidRequest(String),
    MethodNotFound(String),
    InvalidParams(String),
    Internal(String),
    TaskNotFound(String),
    TaskNotCancelable(String),
    PushNotificationNotSupported,
    UnsupportedOperation(String),
    ContentTypeNotSupported(String),
    ExtendedAgentCardNotConfigured,
    VersionNotSupported(String),
}

impl A2aError {
    pub fn code(&self) -> i64 {
        match self {
            A2aError::Parse => -32700,
            A2aError::InvalidRequest(_) => -32600,
            A2aError::MethodNotFound(_) => -32601,
            A2aError::InvalidParams(_) => -32602,
            A2aError::Internal(_) => -32603,
            A2aError::TaskNotFound(_) => -32001,
            A2aError::TaskNotCancelable(_) => -32002,
            A2aError::PushNotificationNotSupported => -32003,
            A2aError::UnsupportedOperation(_) => -32004,
            A2aError::ContentTypeNotSupported(_) => -32005,
            A2aError::ExtendedAgentCardNotConfigured => -32007,
            A2aError::VersionNotSupported(_) => -32009,
        }
    }

    pub fn message(&self) -> String {
        match self {
            A2aError::Parse => "Parse error".into(),
            A2aError::InvalidRequest(why) => format!("Invalid request: {why}"),
            A2aError::MethodNotFound(m) => format!("Method not found: {m}"),
            A2aError::InvalidParams(why) => format!("Invalid params: {why}"),
            A2aError::Internal(why) => format!("Internal error: {why}"),
            A2aError::TaskNotFound(id) => format!("Task not found: {id}"),
            A2aError::TaskNotCancelable(id) => format!("Task {id} has ended and can't be canceled"),
            A2aError::PushNotificationNotSupported => "Push notifications are not supported".into(),
            A2aError::UnsupportedOperation(why) => format!("Unsupported operation: {why}"),
            A2aError::ContentTypeNotSupported(t) => {
                format!("Content type not supported: {t} (send text or application/json)")
            }
            A2aError::ExtendedAgentCardNotConfigured => "No extended agent card".into(),
            A2aError::VersionNotSupported(v) => {
                format!("A2A version {v} is not supported (this agent speaks {PROTOCOL_VERSION})")
            }
        }
    }
}

/// Accepts no header (the spec's default) or a 1.x version.
pub fn check_version(header: Option<&str>) -> Result<(), A2aError> {
    match header.map(str::trim) {
        None | Some("") | Some("1") => Ok(()),
        Some(v) if v == PROTOCOL_VERSION || v.starts_with("1.") => Ok(()),
        Some(v) => Err(A2aError::VersionNotSupported(v.to_string())),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendConfig {
    #[serde(default)]
    pub return_immediately: bool,
    #[serde(default)]
    pub history_length: Option<usize>,
}

/// A JSON-RPC call this agent answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    SendMessage {
        message: Message,
        config: SendConfig,
    },
    GetTask {
        id: String,
        history_length: Option<usize>,
    },
    ListTasks(ListQuery),
    CancelTask {
        id: String,
    },
}

/// A `ListTasks` request: newest first, filtered, one page at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct ListQuery {
    pub context_id: Option<String>,
    pub status: Option<TaskState>,
    /// 1 to 100, 20 when unset.
    pub page_size: usize,
    /// The `nextPageToken` of the page before: the id of its last task.
    pub page_token: Option<String>,
    /// Each task's newest messages to keep; none when unset.
    pub history_length: Option<usize>,
    pub include_artifacts: bool,
}

impl Default for ListQuery {
    fn default() -> ListQuery {
        ListQuery {
            context_id: None,
            status: None,
            page_size: 20,
            page_token: None,
            history_length: None,
            include_artifacts: false,
        }
    }
}

/// One page of `ListTasks`, serialized as its result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPage {
    pub tasks: Vec<Task>,
    /// Empty on the last page.
    pub next_page_token: String,
    pub page_size: usize,
    /// Tasks that match the filters, on every page.
    pub total_size: usize,
}

#[derive(Deserialize)]
struct Envelope {
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Value,
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendParams {
    message: Message,
    #[serde(default)]
    configuration: SendConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskParams {
    id: String,
    #[serde(default)]
    history_length: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ListParams {
    #[serde(default, deserialize_with = "empty_as_unset")]
    context_id: Option<String>,
    #[serde(default)]
    status: Option<TaskState>,
    #[serde(default)]
    page_size: Option<usize>,
    #[serde(default, deserialize_with = "empty_as_unset")]
    page_token: Option<String>,
    #[serde(default)]
    history_length: Option<usize>,
    #[serde(default)]
    include_artifacts: bool,
}

/// Read one JSON-RPC request. The error comes with the request id to answer
/// with (`null` when there isn't one).
pub fn parse_call(body: &[u8]) -> Result<(Value, Call), (Value, A2aError)> {
    let envelope: Envelope =
        serde_json::from_slice(body).map_err(|_| (Value::Null, A2aError::Parse))?;
    let id = envelope.id;
    if envelope.jsonrpc.as_deref() != Some("2.0") {
        return Err((
            id,
            A2aError::InvalidRequest("jsonrpc must be \"2.0\"".into()),
        ));
    }
    let Some(method) = envelope.method else {
        return Err((id, A2aError::InvalidRequest("no method".into())));
    };
    let params = envelope.params;
    let bad = |e: serde_json::Error| A2aError::InvalidParams(e.to_string());
    let call = match method.as_str() {
        "SendMessage" => serde_json::from_value::<SendParams>(params)
            .map_err(bad)
            .and_then(|p| {
                p.message.check()?;
                Ok(Call::SendMessage {
                    message: p.message,
                    config: p.configuration,
                })
            }),
        "GetTask" => serde_json::from_value::<TaskParams>(params)
            .map(|p| Call::GetTask {
                id: p.id,
                history_length: p.history_length,
            })
            .map_err(bad),
        "ListTasks" => {
            let p = if params.is_null() {
                Ok(ListParams::default())
            } else {
                serde_json::from_value::<ListParams>(params).map_err(bad)
            };
            p.map(|p| {
                Call::ListTasks(ListQuery {
                    context_id: p.context_id,
                    status: p.status,
                    page_size: p.page_size.unwrap_or(20).clamp(1, 100),
                    page_token: p.page_token,
                    history_length: p.history_length,
                    include_artifacts: p.include_artifacts,
                })
            })
        }
        "CancelTask" => serde_json::from_value::<TaskParams>(params)
            .map(|p| Call::CancelTask { id: p.id })
            .map_err(bad),
        "SendStreamingMessage" | "SubscribeToTask" => Err(A2aError::UnsupportedOperation(
            "streaming; use SendMessage and GetTask".into(),
        )),
        "CreateTaskPushNotificationConfig"
        | "GetTaskPushNotificationConfig"
        | "ListTaskPushNotificationConfigs"
        | "DeleteTaskPushNotificationConfig" => Err(A2aError::PushNotificationNotSupported),
        "GetExtendedAgentCard" => Err(A2aError::ExtendedAgentCardNotConfigured),
        other => Err(A2aError::MethodNotFound(other.to_string())),
    };
    match call {
        Ok(call) => Ok((id, call)),
        Err(e) => Err((id, e)),
    }
}

/// A JSON-RPC success answer.
pub fn answer(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A JSON-RPC error answer.
pub fn refuse(id: Value, error: &A2aError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": error.code(), "message": error.message() },
    })
}

/// The agent card for one profile, served at `url` over JSON-RPC.
pub fn card(profile: &Profile, url: &str, app_version: &str) -> Value {
    let description = profile
        .about
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} ({} agent)", profile.name, profile.kind.label()));
    let mut tags = vec![Value::from(profile.kind.label())];
    if !profile.team.is_empty() {
        tags.push(Value::from(profile.team.clone()));
    }
    json!({
        "name": profile.name,
        "description": description,
        "version": app_version,
        "supportedInterfaces": [
            { "url": url, "protocolBinding": "JSONRPC", "protocolVersion": PROTOCOL_VERSION }
        ],
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "extendedAgentCard": false,
        },
        "securitySchemes": {
            "hub-token": { "httpAuthSecurityScheme": { "scheme": "Bearer" } }
        },
        "securityRequirements": [{ "schemes": { "hub-token": { "list": [] } } }],
        "defaultInputModes": ["text/plain", "application/json"],
        "defaultOutputModes": ["text/plain"],
        "skills": [{
            "id": profile.id,
            "name": profile.name,
            "description": description,
            "tags": tags,
        }],
    })
}

/// How a working task ended its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Completed(String),
    Failed(String),
    /// The agent asked the sender something; a follow-up message goes on.
    InputRequired(String),
    /// The agent turned the work down.
    Rejected(String),
}

/// Tasks in memory, oldest first. Ended tasks give way to new ones past the cap.
#[derive(Debug, Clone)]
pub struct TaskBook {
    tasks: Vec<Task>,
    cap: usize,
    next: u64,
}

impl TaskBook {
    pub fn new(cap: usize) -> TaskBook {
        TaskBook {
            tasks: Vec::new(),
            cap: cap.max(1),
            next: 1,
        }
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Task, A2aError> {
        self.tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))
    }

    /// A message starts a task, or goes on with one that waits on the sender.
    /// Returns the task's id.
    pub fn open(&mut self, mut message: Message, now_ms: u64) -> Result<String, A2aError> {
        message.check()?;
        message.role = Role::User;
        if let Some(id) = message.task_id.clone() {
            let task = self.find_mut(&id)?;
            if task.status.state.is_terminal() {
                return Err(A2aError::UnsupportedOperation(format!(
                    "task {id} has ended; send a new message in context {}",
                    task.context_id
                )));
            }
            if !task.status.state.is_interrupted() {
                return Err(A2aError::UnsupportedOperation(format!(
                    "task {id} is still working"
                )));
            }
            message.context_id = Some(task.context_id.clone());
            task.history.push(message);
            task.status = TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: stamp(now_ms),
            };
            return Ok(id);
        }
        if self.tasks.len() >= self.cap {
            let Some(at) = self.tasks.iter().position(|t| t.status.state.is_terminal()) else {
                return Err(A2aError::UnsupportedOperation(format!(
                    "{} tasks are already open; wait for one to end",
                    self.cap
                )));
            };
            self.tasks.remove(at);
        }
        let n = self.next;
        self.next += 1;
        let id = format!("task-{n}");
        let context_id = message
            .context_id
            .clone()
            .unwrap_or_else(|| format!("ctx-{n}"));
        message.context_id = Some(context_id.clone());
        message.task_id = Some(id.clone());
        self.tasks.push(Task {
            id: id.clone(),
            context_id,
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: stamp(now_ms),
            },
            artifacts: Vec::new(),
            history: vec![message],
        });
        Ok(id)
    }

    /// The receiving agent starts on a submitted task.
    pub fn start(&mut self, id: &str, now_ms: u64) -> Result<(), A2aError> {
        let task = self.find_mut(id)?;
        if task.status.state != TaskState::Submitted {
            return Err(A2aError::UnsupportedOperation(format!(
                "task {id} isn't waiting to start"
            )));
        }
        task.status = TaskStatus {
            state: TaskState::Working,
            message: None,
            timestamp: stamp(now_ms),
        };
        Ok(())
    }

    /// A working task's turn ended. A task canceled meanwhile stays canceled.
    pub fn finish(&mut self, id: &str, outcome: Outcome, now_ms: u64) -> Result<(), A2aError> {
        let task = self.find_mut(id)?;
        if task.status.state == TaskState::Canceled {
            return Ok(());
        }
        if task.status.state != TaskState::Working {
            return Err(A2aError::UnsupportedOperation(format!(
                "task {id} isn't working"
            )));
        }
        let turn = task.history.len();
        let reply_id = format!("{id}-reply-{turn}");
        let (state, text) = match outcome {
            Outcome::Completed(text) => {
                task.artifacts = vec![Artifact {
                    artifact_id: format!("{id}-reply"),
                    name: "reply".into(),
                    parts: vec![Part::Text(text.clone())],
                }];
                (TaskState::Completed, text)
            }
            Outcome::Failed(why) => (TaskState::Failed, why),
            Outcome::InputRequired(question) => (TaskState::InputRequired, question),
            Outcome::Rejected(why) => (TaskState::Rejected, why),
        };
        let mut reply = Message::text(Role::Agent, &reply_id, &text);
        reply.context_id = Some(task.context_id.clone());
        reply.task_id = Some(id.to_string());
        task.history.push(reply.clone());
        task.status = TaskStatus {
            state,
            message: Some(reply),
            timestamp: stamp(now_ms),
        };
        Ok(())
    }

    /// Only a task that hasn't ended can be canceled.
    pub fn cancel(&mut self, id: &str, now_ms: u64) -> Result<Task, A2aError> {
        let task = self.find_mut(id)?;
        if task.status.state.is_terminal() {
            return Err(A2aError::TaskNotCancelable(id.to_string()));
        }
        task.status = TaskStatus {
            state: TaskState::Canceled,
            message: None,
            timestamp: stamp(now_ms),
        };
        Ok(task.clone())
    }

    /// A copy with at most `history_length` of the newest messages.
    pub fn get(&self, id: &str, history_length: Option<usize>) -> Result<Task, A2aError> {
        let task = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))?;
        let mut task = task.clone();
        if let Some(n) = history_length {
            keep_newest(&mut task.history, n);
        }
        Ok(task)
    }

    /// One page of the tasks that match `query`, newest first. A task's
    /// history is left out unless `history_length` asks for some, and its
    /// artifacts unless `include_artifacts`.
    pub fn list(&self, query: &ListQuery) -> Result<TaskPage, A2aError> {
        let matching: Vec<&Task> = self
            .tasks
            .iter()
            .rev()
            .filter(|t| query.context_id.as_ref().is_none_or(|c| &t.context_id == c))
            .filter(|t| query.status.is_none_or(|s| t.status.state == s))
            .collect();
        let start = match &query.page_token {
            None => 0,
            Some(token) => {
                matching
                    .iter()
                    .position(|t| &t.id == token)
                    .ok_or_else(|| {
                        A2aError::InvalidParams(format!("pageToken `{token}` isn't a listed task"))
                    })?
                    + 1
            }
        };
        let page: Vec<&Task> = matching
            .iter()
            .skip(start)
            .take(query.page_size)
            .copied()
            .collect();
        let more = start + page.len() < matching.len();
        let next_page_token = match page.last() {
            Some(last) if more => last.id.clone(),
            _ => String::new(),
        };
        let tasks = page
            .into_iter()
            .map(|t| {
                let mut task = t.clone();
                keep_newest(&mut task.history, query.history_length.unwrap_or(0));
                if !query.include_artifacts {
                    task.artifacts.clear();
                }
                task
            })
            .collect();
        Ok(TaskPage {
            tasks,
            next_page_token,
            page_size: query.page_size,
            total_size: matching.len(),
        })
    }
}

/// Drops all but the newest `n` messages.
fn keep_newest(history: &mut Vec<Message>, n: usize) {
    let skip = history.len().saturating_sub(n);
    history.drain(..skip);
}

/// `2026-11-25T01:02:03.004Z` for a Unix time in milliseconds.
pub fn stamp(now_ms: u64) -> String {
    let secs = (now_ms / 1000) as i64;
    let (y, m, d) = crate::objectives::season::civil(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        now_ms % 1000
    )
}

#[cfg(test)]
#[path = "a2a_tests.rs"]
mod tests;
