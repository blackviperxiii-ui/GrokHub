//! Native tool definitions for the registry: name, short index line, and schema builder.

use serde_json::Value;

use crate::gate::Gate;

use super::{
    connections, control, desktop, glob, grep, list_dir, media, read_file, search_replace, shell,
    web_fetch, write,
};

/// One row in the native catalog. `defer` tools ship as index lines until
/// [`super::registry::load_tool_schema`] loads the full JSON schema into the volatile tail.
#[derive(Clone, Copy)]
pub struct ToolDef {
    pub name: &'static str,
    pub blurb: &'static str,
    pub defer: bool,
    pub schema: fn() -> Value,
    pub visible: fn(&Gate) -> bool,
}

fn always(_: &Gate) -> bool {
    true
}

fn not_readonly(gate: &Gate) -> bool {
    !gate.readonly_session
}

fn desktop_on(gate: &Gate) -> bool {
    gate.desktop
}

fn media_schema(i: usize) -> Value {
    media::schemas()[i].clone()
}

fn desktop_schema(i: usize) -> Value {
    desktop::schemas()[i].clone()
}

/// Read-only tools always sent with full schemas (even when deferred loading is on).
pub fn eager_defs() -> &'static [ToolDef] {
    static DEFS: [ToolDef; 8] = [
        ToolDef {
            name: "read_file",
            blurb: "Read a workspace file",
            defer: false,
            schema: read_file::schema,
            visible: always,
        },
        ToolDef {
            name: "list_dir",
            blurb: "List a workspace directory",
            defer: false,
            schema: list_dir::schema,
            visible: always,
        },
        ToolDef {
            name: "grep",
            blurb: "Search file contents in the workspace",
            defer: false,
            schema: grep::schema,
            visible: always,
        },
        ToolDef {
            name: "glob",
            blurb: "Find paths by glob in the workspace",
            defer: false,
            schema: glob::schema,
            visible: always,
        },
        ToolDef {
            name: "get_command_or_subagent_output",
            blurb: "Read background command or subagent output",
            defer: false,
            schema: control::output_schema,
            visible: always,
        },
        ToolDef {
            name: "scheduler_list",
            blurb: "List scheduled automations",
            defer: false,
            schema: control::scheduler_list_schema,
            visible: always,
        },
        ToolDef {
            name: "skill",
            blurb: "Load a cabin skill by name",
            defer: false,
            schema: || crate::skills::schema(),
            visible: always,
        },
        ToolDef {
            name: "diagnose",
            blurb: "Run a read-only system diagnose probe",
            defer: false,
            schema: || crate::repair::schema(),
            visible: always,
        },
    ];
    &DEFS
}

/// Tools that may be deferred (full schema only after `load_tool_schema`).
pub fn deferred_native_defs() -> &'static [ToolDef] {
    static DEFS: [ToolDef; 18] = [
        ToolDef {
            name: "send_subagent_message",
            blurb: "Send a message to a spawned subagent",
            defer: true,
            schema: || crate::subagent::send_schema(),
            visible: not_readonly,
        },
        ToolDef {
            name: "write",
            blurb: "Create or overwrite a workspace file",
            defer: true,
            schema: write::schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "search_replace",
            blurb: "Replace text in a workspace file",
            defer: true,
            schema: search_replace::schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "run_terminal_command",
            blurb: "Run a shell command in the workspace",
            defer: true,
            schema: shell::schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "web_fetch",
            blurb: "Fetch a public URL",
            defer: true,
            schema: web_fetch::schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "image_generate",
            blurb: "Generate an image with Imagine",
            defer: true,
            schema: || media_schema(0),
            visible: not_readonly,
        },
        ToolDef {
            name: "image_edit",
            blurb: "Edit an image with Imagine",
            defer: true,
            schema: || media_schema(1),
            visible: not_readonly,
        },
        ToolDef {
            name: "video_generate",
            blurb: "Generate a short video with Imagine",
            defer: true,
            schema: || media_schema(2),
            visible: not_readonly,
        },
        ToolDef {
            name: "video_edit",
            blurb: "Edit a video with Imagine",
            defer: true,
            schema: || media_schema(3),
            visible: not_readonly,
        },
        ToolDef {
            name: "video_extend",
            blurb: "Extend a video with Imagine",
            defer: true,
            schema: || media_schema(4),
            visible: not_readonly,
        },
        ToolDef {
            name: "kill_command_or_subagent",
            blurb: "Stop a background command or subagent",
            defer: true,
            schema: control::kill_schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "monitor",
            blurb: "Watch a background command",
            defer: true,
            schema: control::monitor_schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "scheduler_create",
            blurb: "Create a scheduled automation",
            defer: true,
            schema: control::scheduler_create_schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "scheduler_delete",
            blurb: "Delete a scheduled automation",
            defer: true,
            schema: control::scheduler_delete_schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "connection_delete",
            blurb: "Remove an MCP connection entry",
            defer: true,
            schema: connections::delete_schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "screenshot",
            blurb: "Capture the desktop screen",
            defer: true,
            schema: || desktop_schema(0),
            visible: desktop_on,
        },
        ToolDef {
            name: "click",
            blurb: "Click on the desktop",
            defer: true,
            schema: || desktop_schema(1),
            visible: desktop_on,
        },
        ToolDef {
            name: "move",
            blurb: "Move the desktop pointer",
            defer: true,
            schema: || desktop_schema(2),
            visible: desktop_on,
        },
    ];
    &DEFS
}

/// Remaining desktop tools (deferred as a group when desktop is on).
pub fn deferred_desktop_tail() -> &'static [ToolDef] {
    static DEFS: [ToolDef; 4] = [
        ToolDef {
            name: "drag",
            blurb: "Drag on the desktop",
            defer: true,
            schema: || desktop_schema(3),
            visible: desktop_on,
        },
        ToolDef {
            name: "scroll",
            blurb: "Scroll on the desktop",
            defer: true,
            schema: || desktop_schema(4),
            visible: desktop_on,
        },
        ToolDef {
            name: "type",
            blurb: "Type text on the desktop",
            defer: true,
            schema: || desktop_schema(5),
            visible: desktop_on,
        },
        ToolDef {
            name: "key",
            blurb: "Press a key on the desktop",
            defer: true,
            schema: || desktop_schema(6),
            visible: desktop_on,
        },
    ];
    &DEFS
}

pub fn find_def(name: &str) -> Option<ToolDef> {
    eager_defs()
        .iter()
        .chain(deferred_native_defs())
        .chain(deferred_desktop_tail())
        .find(|d| d.name == name)
        .copied()
}
