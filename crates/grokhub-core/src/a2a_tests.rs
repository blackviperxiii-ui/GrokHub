use super::*;
use crate::agent_team::{Kind, Profile};

const T0: u64 = 1_795_000_000_000;

fn ask(text: &str) -> Message {
    Message::text(Role::User, "m-1", text)
}

#[test]
fn shapes_serialize_with_the_spec_names() {
    let mut message = Message::text(Role::Agent, "m-9", "done");
    message.context_id = Some("ctx-1".into());
    message.parts.push(Part::Data {
        data: json!({ "rows": 2 }),
        media_type: "application/json".into(),
    });
    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        json!({
            "messageId": "m-9",
            "contextId": "ctx-1",
            "role": "ROLE_AGENT",
            "parts": [{ "text": "done" }, { "data": { "rows": 2 }, "mediaType": "application/json" }],
        })
    );
    assert_eq!(
        serde_json::to_value(TaskState::InputRequired).unwrap(),
        json!("TASK_STATE_INPUT_REQUIRED")
    );
    let back: Message = serde_json::from_value(serde_json::to_value(&message).unwrap()).unwrap();
    assert_eq!(back, message);
}

#[test]
fn a_file_part_parses_but_never_crosses() {
    let message: Message = serde_json::from_value(json!({
        "messageId": "m-1",
        "role": "ROLE_USER",
        "parts": [{ "url": "https://example.org/a.pdf", "filename": "a.pdf", "mediaType": "application/pdf" }],
    }))
    .unwrap();
    assert_eq!(
        message.parts,
        vec![Part::File {
            filename: "a.pdf".into(),
            media_type: "application/pdf".into()
        }]
    );
    assert_eq!(
        message.check(),
        Err(A2aError::ContentTypeNotSupported("application/pdf".into()))
    );
    let bad: Result<Message, _> = serde_json::from_value(json!({
        "messageId": "m-1", "role": "ROLE_USER", "parts": [{ "kind": "text" }],
    }));
    assert!(bad
        .unwrap_err()
        .to_string()
        .contains("a part has text, data, raw or url"));
}

#[test]
fn a_message_is_capped_in_parts_and_bytes() {
    assert_eq!(ask("hi").check(), Ok(()));
    let mut empty = ask("hi");
    empty.parts.clear();
    assert_eq!(
        empty.check(),
        Err(A2aError::InvalidParams(
            "a message has 1 to 16 parts".into()
        ))
    );
    assert_eq!(
        ask(&"x".repeat(MAX_PART_BYTES + 1)).check(),
        Err(A2aError::InvalidParams(
            "a part is at most 32768 bytes".into()
        ))
    );
    let mut wide = ask(&"x".repeat(MAX_PART_BYTES));
    wide.parts.push(Part::Text("x".repeat(MAX_PART_BYTES)));
    wide.parts.push(Part::Text("x".into()));
    assert_eq!(
        wide.check(),
        Err(A2aError::InvalidParams(
            "a message is at most 65536 bytes".into()
        ))
    );
    let mut csv = ask("hi");
    csv.parts = vec![Part::Data {
        data: json!("a,b"),
        media_type: "text/csv".into(),
    }];
    assert_eq!(
        csv.check(),
        Err(A2aError::ContentTypeNotSupported("text/csv".into()))
    );
    let mut no_id = ask("hi");
    no_id.message_id = " ".into();
    assert_eq!(
        no_id.check(),
        Err(A2aError::InvalidParams(
            "messageId is 1 to 128 characters".into()
        ))
    );
    let mut long_context = ask("hi");
    long_context.context_id = Some("c".repeat(129));
    assert_eq!(
        long_context.check(),
        Err(A2aError::InvalidParams(
            "contextId is 1 to 128 characters".into()
        ))
    );
    long_context.context_id = Some("c".repeat(128));
    assert_eq!(long_context.check(), Ok(()));
    let mut blank_task = ask("hi");
    blank_task.task_id = Some(" ".into());
    assert_eq!(
        blank_task.check(),
        Err(A2aError::InvalidParams(
            "taskId is 1 to 128 characters".into()
        ))
    );
    // ProtoJSON sends an unset id as "".
    let unset: Message = serde_json::from_value(json!({
        "messageId": "m-1",
        "contextId": "",
        "taskId": "",
        "role": "ROLE_USER",
        "parts": [{ "text": "hi" }],
    }))
    .unwrap();
    assert_eq!(unset.check(), Ok(()));
    assert_eq!((unset.context_id, unset.task_id), (None, None));
}

#[test]
fn plain_text_joins_text_and_data_for_the_prompt() {
    let mut message = ask("Count these:");
    message.parts.push(Part::Data {
        data: json!([1, 2]),
        media_type: "application/json".into(),
    });
    assert_eq!(message.plain_text(), "Count these:\n\n[1,2]");
}

#[test]
fn a_task_runs_submitted_working_completed_and_then_takes_no_more() {
    let mut book = TaskBook::new(10);
    let id = book.open(ask("Find the router manual"), T0).unwrap();
    assert_eq!(id, "task-1");
    let task = book.get(&id, None).unwrap();
    assert_eq!(task.context_id, "ctx-1");
    assert_eq!(task.status.state, TaskState::Submitted);
    assert_eq!(task.status.timestamp, "2026-11-18T11:06:40.000Z");
    assert_eq!(task.history[0].task_id.as_deref(), Some("task-1"));

    book.start(&id, T0 + 1).unwrap();
    assert_eq!(
        book.get(&id, None).unwrap().status.state,
        TaskState::Working
    );
    book.finish(&id, Outcome::Completed("It's in the box.".into()), T0 + 2)
        .unwrap();

    let done = book.get(&id, None).unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    assert_eq!(done.reply().as_deref(), Some("It's in the box."));
    assert_eq!(done.status.message.as_ref().unwrap().role, Role::Agent);
    assert_eq!(
        done.status.message.as_ref().unwrap().message_id,
        "task-1-reply-1"
    );
    assert_eq!(done.history.len(), 2);

    let mut more = ask("And the warranty?");
    more.task_id = Some(id.clone());
    assert_eq!(
        book.open(more, T0 + 3),
        Err(A2aError::UnsupportedOperation(
            "task task-1 has ended; send a new message in context ctx-1".into()
        ))
    );
    assert_eq!(
        book.cancel(&id, T0 + 4),
        Err(A2aError::TaskNotCancelable("task-1".into()))
    );
}

#[test]
fn an_agent_that_asks_back_gets_the_answer_on_the_same_task() {
    let mut book = TaskBook::new(10);
    let id = book.open(ask("Book a table"), T0).unwrap();
    book.start(&id, T0).unwrap();
    book.finish(&id, Outcome::InputRequired("For how many?".into()), T0)
        .unwrap();
    assert_eq!(
        book.get(&id, None).unwrap().status.state,
        TaskState::InputRequired
    );

    let mut answer = Message::text(Role::Agent, "m-2", "Four");
    answer.task_id = Some(id.clone());
    assert_eq!(book.open(answer, T0 + 5), Ok(id.clone()));
    let task = book.get(&id, None).unwrap();
    assert_eq!(task.status.state, TaskState::Submitted);
    // The follow-up is stored as the sender's turn, in the task's context.
    assert_eq!(task.history[2].role, Role::User);
    assert_eq!(task.history[2].context_id.as_deref(), Some("ctx-1"));
    assert_eq!(book.get(&id, Some(1)).unwrap().history.len(), 1);

    // A message to a task that is still working is refused.
    book.start(&id, T0 + 6).unwrap();
    let mut early = ask("Make it five");
    early.task_id = Some(id.clone());
    assert_eq!(
        book.open(early, T0 + 7),
        Err(A2aError::UnsupportedOperation(
            "task task-1 is still working".into()
        ))
    );
}

#[test]
fn a_canceled_task_stays_canceled_when_its_turn_ends() {
    let mut book = TaskBook::new(10);
    let id = book.open(ask("Long job"), T0).unwrap();
    book.start(&id, T0).unwrap();
    assert_eq!(
        book.cancel(&id, T0 + 1).unwrap().status.state,
        TaskState::Canceled
    );
    book.finish(&id, Outcome::Completed("late".into()), T0 + 2)
        .unwrap();
    let task = book.get(&id, None).unwrap();
    assert_eq!(task.status.state, TaskState::Canceled);
    assert_eq!(task.reply(), None);
    assert_eq!(
        book.get("task-9", None),
        Err(A2aError::TaskNotFound("task-9".into()))
    );
}

#[test]
fn ended_tasks_give_way_and_open_ones_never_do() {
    let mut book = TaskBook::new(2);
    let a = book.open(ask("a"), T0).unwrap();
    let _b = book.open(ask("b"), T0).unwrap();
    assert_eq!(
        book.open(ask("c"), T0),
        Err(A2aError::UnsupportedOperation(
            "2 tasks are already open; wait for one to end".into()
        ))
    );
    book.start(&a, T0).unwrap();
    book.finish(&a, Outcome::Failed("no network".into()), T0)
        .unwrap();
    assert_eq!(book.open(ask("c"), T0), Ok("task-3".into()));
    assert_eq!(
        book.get(&a, None),
        Err(A2aError::TaskNotFound("task-1".into()))
    );
    let all = book.list(&ListQuery::default()).unwrap();
    let listed: Vec<&str> = all.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(listed, vec!["task-3", "task-2"]);
    assert!(all.tasks.iter().all(|t| t.history.is_empty()));
    let in_context = ListQuery {
        context_id: Some("ctx-2".into()),
        ..ListQuery::default()
    };
    assert_eq!(book.list(&in_context).unwrap().total_size, 1);
}

#[test]
fn a_task_list_pages_by_token_and_filters_by_state() {
    let mut book = TaskBook::new(8);
    for word in ["a", "b", "c"] {
        let id = book.open(ask(word), T0).unwrap();
        book.start(&id, T0).unwrap();
        book.finish(&id, Outcome::Completed(format!("re {word}")), T0)
            .unwrap();
    }
    let working = book.open(ask("d"), T0).unwrap();
    book.start(&working, T0).unwrap();
    let ids =
        |page: &TaskPage| -> Vec<String> { page.tasks.iter().map(|t| t.id.clone()).collect() };
    let first = book
        .list(&ListQuery {
            page_size: 3,
            ..ListQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&first), ["task-4", "task-3", "task-2"]);
    assert_eq!(
        (
            first.next_page_token.as_str(),
            first.page_size,
            first.total_size
        ),
        ("task-2", 3, 4)
    );
    assert!(first.tasks.iter().all(|t| t.artifacts.is_empty()));
    let second = book
        .list(&ListQuery {
            page_size: 3,
            page_token: Some(first.next_page_token.clone()),
            include_artifacts: true,
            history_length: Some(1),
            ..ListQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&second), ["task-1"]);
    assert_eq!(second.next_page_token, "");
    assert_eq!(second.tasks[0].reply().as_deref(), Some("re a"));
    assert_eq!(second.tasks[0].history.len(), 1);
    let done = book
        .list(&ListQuery {
            status: Some(TaskState::Working),
            ..ListQuery::default()
        })
        .unwrap();
    assert_eq!((ids(&done), done.total_size), (vec![working], 1));
    assert_eq!(
        book.list(&ListQuery {
            page_token: Some("task-9".into()),
            ..ListQuery::default()
        }),
        Err(A2aError::InvalidParams(
            "pageToken `task-9` isn't a listed task".into()
        ))
    );
    let page = serde_json::to_value(&first).unwrap();
    assert_eq!(page["nextPageToken"], "task-2");
    assert_eq!(
        (page["pageSize"].clone(), page["totalSize"].clone()),
        (json!(3), json!(4))
    );
}

#[test]
fn json_rpc_calls_parse_and_the_rest_answer_with_spec_codes() {
    let body = json!({
        "jsonrpc": "2.0", "id": 7, "method": "SendMessage",
        "params": { "message": { "messageId": "m-1", "role": "ROLE_USER", "parts": [{ "text": "hi" }] },
                    "configuration": { "returnImmediately": true } },
    });
    let (id, call) = parse_call(body.to_string().as_bytes()).unwrap();
    assert_eq!(id, json!(7));
    assert_eq!(
        call,
        Call::SendMessage {
            message: ask("hi"),
            config: SendConfig {
                return_immediately: true,
                history_length: None
            },
        }
    );
    let get = json!({ "jsonrpc": "2.0", "id": "a", "method": "GetTask", "params": { "id": "task-1", "historyLength": 2 } });
    assert_eq!(
        parse_call(get.to_string().as_bytes()).unwrap().1,
        Call::GetTask {
            id: "task-1".into(),
            history_length: Some(2)
        }
    );
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "ListTasks" });
    assert_eq!(
        parse_call(list.to_string().as_bytes()).unwrap().1,
        Call::ListTasks(ListQuery::default())
    );
    let filtered = json!({
        "jsonrpc": "2.0", "id": 1, "method": "ListTasks",
        "params": {
            "contextId": "", "status": "TASK_STATE_WORKING", "pageSize": 500,
            "pageToken": "task-2", "historyLength": 3, "includeArtifacts": true
        }
    });
    assert_eq!(
        parse_call(filtered.to_string().as_bytes()).unwrap().1,
        Call::ListTasks(ListQuery {
            context_id: None,
            status: Some(TaskState::Working),
            page_size: 100,
            page_token: Some("task-2".into()),
            history_length: Some(3),
            include_artifacts: true,
        })
    );

    let code = |body: Value| {
        parse_call(body.to_string().as_bytes())
            .unwrap_err()
            .1
            .code()
    };
    assert_eq!(
        parse_call(b"{nope").unwrap_err(),
        (Value::Null, A2aError::Parse)
    );
    assert_eq!(
        code(json!({ "jsonrpc": "1.0", "id": 1, "method": "GetTask" })),
        -32600
    );
    assert_eq!(
        code(json!({ "jsonrpc": "2.0", "id": 1, "method": "Shutdown" })),
        -32601
    );
    assert_eq!(
        code(json!({ "jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {} })),
        -32602
    );
    assert_eq!(
        code(json!({ "jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage" })),
        -32004
    );
    assert_eq!(
        code(json!({ "jsonrpc": "2.0", "id": 1, "method": "CreateTaskPushNotificationConfig" })),
        -32003
    );
    assert_eq!(
        code(json!({ "jsonrpc": "2.0", "id": 1, "method": "GetExtendedAgentCard" })),
        -32007
    );
    let pdf = json!({
        "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
        "params": { "message": { "messageId": "m", "role": "ROLE_USER", "parts": [{ "raw": "JVBE", "mediaType": "application/pdf" }] } },
    });
    assert_eq!(code(pdf), -32005);

    assert_eq!(
        refuse(json!(3), &A2aError::TaskNotFound("task-4".into())),
        json!({ "jsonrpc": "2.0", "id": 3, "error": { "code": -32001, "message": "Task not found: task-4" } })
    );
    assert_eq!(
        answer(json!(3), json!({ "ok": true })),
        json!({ "jsonrpc": "2.0", "id": 3, "result": { "ok": true } })
    );
}

#[test]
fn a2a_version_one_or_none_is_accepted() {
    assert_eq!(check_version(None), Ok(()));
    assert_eq!(check_version(Some("1.0")), Ok(()));
    assert_eq!(check_version(Some("1.2")), Ok(()));
    assert_eq!(
        check_version(Some("0.3")),
        Err(A2aError::VersionNotSupported("0.3".into()))
    );
    assert_eq!(
        A2aError::VersionNotSupported("0.3".into()).message(),
        "A2A version 0.3 is not supported (this agent speaks 1.0)"
    );
}

#[test]
fn a_profile_card_has_the_required_fields_and_one_skill() {
    let mut desk = Profile::new(Kind::Customer, "front-desk", "Front desk");
    desk.team = "shop".into();
    desk.about = "\nAnswers opening hours and order status.\nNever shares other orders.".into();
    let card = card(&desk, "http://127.0.0.1:18766/a2a/v1/front-desk", "3.6.0");
    assert_eq!(
        card,
        json!({
            "name": "Front desk",
            "description": "Answers opening hours and order status.",
            "version": "3.6.0",
            "supportedInterfaces": [{
                "url": "http://127.0.0.1:18766/a2a/v1/front-desk",
                "protocolBinding": "JSONRPC",
                "protocolVersion": "1.0",
            }],
            "capabilities": { "streaming": false, "pushNotifications": false, "extendedAgentCard": false },
            "securitySchemes": {
                "hub-token": { "httpAuthSecurityScheme": { "scheme": "Bearer" } }
            },
            "securityRequirements": [{ "schemes": { "hub-token": { "list": [] } } }],
            "defaultInputModes": ["text/plain", "application/json"],
            "defaultOutputModes": ["text/plain"],
            "skills": [{
                "id": "front-desk",
                "name": "Front desk",
                "description": "Answers opening hours and order status.",
                "tags": ["customer", "shop"],
            }],
        })
    );
    let bare = Profile::new(Kind::Role, "researcher", "Researcher");
    assert_eq!(
        super::card(&bare, "u", "1")["description"],
        json!("Researcher (role agent)")
    );
}

#[test]
fn stamps_are_utc_with_milliseconds() {
    assert_eq!(stamp(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(stamp(951_782_400_123), "2000-02-29T00:00:00.123Z");
    assert_eq!(stamp(1_795_000_000_000), "2026-11-18T11:06:40.000Z");
}
