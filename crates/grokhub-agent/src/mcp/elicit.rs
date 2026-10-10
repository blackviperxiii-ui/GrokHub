//! Map an MCP `elicitation/create` request onto the cabin `ElicitAsk` card.
//! Unattended runs decline without showing the card.

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

use crate::CancelToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitView {
    pub id: String,
    pub server_name: String,
    pub message: String,
    pub mode: String,
    pub url: String,
    pub elicitation_id: String,
    pub field_name: Option<String>,
    pub field_title: String,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitNote {
    pub id: String,
    pub action: String,
    pub content: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElicitAnswer {
    Accept(Value),
    Decline,
    Cancel,
}

pub struct ElicitInbox {
    rx: Mutex<Receiver<ElicitNote>>,
}

impl ElicitInbox {
    pub fn pair() -> (Sender<ElicitNote>, Self) {
        let (tx, rx) = mpsc::channel();
        (tx, Self { rx: Mutex::new(rx) })
    }
}

/// Stack frame for one `with_elicit` call. The closures stay on the caller's stack.
/// Only this pointer is stored in the thread-local, so the closures need not be `'static`.
struct HookFrame {
    attended: bool,
    emit: *mut (),
    wait: *mut (),
    emit_fn: fn(*mut (), ElicitView),
    wait_fn: fn(*mut (), &str) -> ElicitAnswer,
}

thread_local! {
    static HOOK: Cell<*const HookFrame> = const { Cell::new(std::ptr::null()) };
}

pub(crate) fn with_elicit<R, E, W>(
    attended: bool,
    emit: &mut E,
    wait: &mut W,
    body: impl FnOnce() -> R,
) -> R
where
    E: FnMut(ElicitView),
    W: FnMut(&str) -> ElicitAnswer,
{
    fn call_emit<E: FnMut(ElicitView)>(ptr: *mut (), view: ElicitView) {
        // SAFETY: `ptr` is the `&mut E` installed by `with_elicit` on this thread.
        unsafe { (*ptr.cast::<E>())(view) }
    }
    fn call_wait<W: FnMut(&str) -> ElicitAnswer>(ptr: *mut (), id: &str) -> ElicitAnswer {
        // SAFETY: `ptr` is the `&mut W` installed by `with_elicit` on this thread.
        unsafe { (*ptr.cast::<W>())(id) }
    }
    let frame = HookFrame {
        attended,
        emit: std::ptr::from_mut(emit).cast(),
        wait: std::ptr::from_mut(wait).cast(),
        emit_fn: call_emit::<E>,
        wait_fn: call_wait::<W>,
    };
    HOOK.with(|slot| {
        let prev = slot.replace(&frame);
        struct Guard(*const HookFrame);
        impl Drop for Guard {
            fn drop(&mut self) {
                HOOK.with(|slot| slot.set(self.0));
            }
        }
        let _guard = Guard(prev);
        body()
    })
}

fn inboxes() -> &'static Mutex<std::collections::HashMap<String, Arc<ElicitInbox>>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<String, Arc<ElicitInbox>>>> =
        OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub fn attach_elicit(session: &str, inbox: ElicitInbox) {
    let mut map = inboxes().lock().unwrap_or_else(|err| err.into_inner());
    map.insert(session.to_string(), Arc::new(inbox));
}

pub fn detach_elicit(session: &str) {
    let mut map = inboxes().lock().unwrap_or_else(|err| err.into_inner());
    map.remove(session);
}

/// A child session reads the parent's inbox. The map stores `Arc` so both ids
/// share one receiver without holding the map lock across the wait.
pub fn alias_elicit(child: &str, parent: &str) {
    if child.is_empty() || parent.is_empty() || child == parent {
        return;
    }
    let mut map = inboxes().lock().unwrap_or_else(|err| err.into_inner());
    let Some(inbox) = map.get(parent).cloned() else {
        return;
    };
    map.insert(child.to_string(), inbox);
}

pub fn unalias_elicit(child: &str) {
    if child.is_empty() {
        return;
    }
    let mut map = inboxes().lock().unwrap_or_else(|err| err.into_inner());
    map.remove(child);
}

pub(crate) fn wait_elicit(
    session: &str,
    id: &str,
    cancel: &CancelToken,
    halted: &dyn Fn() -> bool,
) -> ElicitAnswer {
    loop {
        if cancel.is_cancelled() {
            return ElicitAnswer::Cancel;
        }
        if halted() {
            return ElicitAnswer::Cancel;
        }
        let inbox = {
            let map = inboxes().lock().unwrap_or_else(|err| err.into_inner());
            let Some(inbox) = map.get(session).cloned() else {
                return ElicitAnswer::Decline;
            };
            inbox
        };
        let rx = inbox.rx.lock().unwrap_or_else(|err| err.into_inner());
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(note) if note.id == id => return note_to_answer(note),
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return ElicitAnswer::Cancel,
        }
    }
}

fn note_to_answer(note: ElicitNote) -> ElicitAnswer {
    match note.action.to_ascii_lowercase().as_str() {
        "accept" => ElicitAnswer::Accept(note.content.unwrap_or_else(|| json!({}))),
        "cancel" => ElicitAnswer::Cancel,
        _ => ElicitAnswer::Decline,
    }
}

pub(crate) fn answer_elicitation(server: &str, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(|m| m.as_str())?;
    if method != "elicitation/create" {
        return None;
    }
    let id = msg.get("id").cloned()?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let answer = ask(server, &id, &params);
    let result = match answer {
        ElicitAnswer::Accept(content) => json!({"action": "accept", "content": content}),
        ElicitAnswer::Cancel => json!({"action": "cancel"}),
        ElicitAnswer::Decline => json!({"action": "decline"}),
    };
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

fn ask(server: &str, id: &Value, params: &Value) -> ElicitAnswer {
    let attended = HOOK.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            false
        } else {
            // SAFETY: the frame lives on this thread's stack for the whole `with_elicit` body.
            unsafe { (*ptr).attended }
        }
    });
    if !attended {
        return ElicitAnswer::Decline;
    }
    let view = view_for(server, id, params);
    let wait_id = view.id.clone();
    let emitted = HOOK.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            false
        } else {
            // SAFETY: same stack frame as above, and `with_elicit` has not returned.
            let (emit_fn, emit_ptr) = unsafe { ((*ptr).emit_fn, (*ptr).emit) };
            emit_fn(emit_ptr, view);
            true
        }
    });
    if !emitted {
        return ElicitAnswer::Decline;
    }
    HOOK.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            ElicitAnswer::Decline
        } else {
            // SAFETY: same stack frame. The wait callback does not re-enter this frame.
            let (wait_fn, wait_ptr) = unsafe { ((*ptr).wait_fn, (*ptr).wait) };
            wait_fn(wait_ptr, &wait_id)
        }
    })
}

/// Ask the user for one secret value on the cabin's elicit card (masked
/// field). `None` when the run is unattended, there is no card, or the user
/// declines. The value goes straight back to the caller: it is never logged.
pub(crate) fn ask_secret(server: &str, message: &str, title: &str) -> Option<String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = json!(format!("{server}-secret-{nanos}"));
    let params = json!({
        "message": message,
        "requestedSchema": {
            "type": "object",
            "properties": {
                "token": {"type": "string", "title": title, "format": "password", "writeOnly": true}
            },
            "required": ["token"]
        }
    });
    match ask(server, &id, &params) {
        ElicitAnswer::Accept(content) => content
            .get("token")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn view_for(server: &str, id: &Value, params: &Value) -> ElicitView {
    let wait_id = id_string(id);
    let mut shaped = params.clone();
    if let Some(obj) = shaped.as_object_mut() {
        obj.entry("serverName".to_string())
            .or_insert_with(|| json!(server));
    }
    let parsed = grokhub_core::wire::parse_elicit(Value::String(wait_id.clone()), &shaped);
    ElicitView {
        id: wait_id,
        server_name: if parsed.server_name.is_empty() {
            server.to_string()
        } else {
            parsed.server_name
        },
        message: parsed.message,
        mode: parsed.mode,
        url: parsed.url,
        elicitation_id: parsed.elicitation_id,
        field_name: parsed.field_name,
        field_title: parsed.field_title,
        secret: parsed.secret,
    }
}

pub(crate) fn id_string(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
