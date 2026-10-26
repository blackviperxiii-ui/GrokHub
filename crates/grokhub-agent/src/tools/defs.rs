//! Native tool definitions for the registry: name, short index line, and schema builder.

use serde_json::Value;

use crate::gate::Gate;

use super::{
    connections, control, desktop, glob, grep, list_dir, media, read_file, search_replace, shell,
    show_panel, web_fetch, write,
};

/// One row in the native catalog. When deferred loading is on, `defer` tools ship as
/// one index line each until `load_tool_schema` loads the full JSON schema into the
/// volatile tail; the rest are sent eagerly. The three lists below only fix the
/// legacy (deferred off) order, which must stay byte-identical.
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

fn generative_ui_on(_gate: &Gate) -> bool {
    show_panel::enabled(&crate::perm::config_dir())
}

fn media_schema(i: usize) -> Value {
    media::schemas()[i].clone()
}

fn desktop_schema(i: usize) -> Value {
    desktop::schemas()[i].clone()
}

/// Read-only tools, sent first. All eager except `scheduler_list`.
pub fn read_defs() -> &'static [ToolDef] {
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
            defer: true,
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

/// Mutating and gated tools. Edit and shell stay eager (the general coding set);
/// scheduler, media, desktop, connection and the rest are deferred.
pub fn action_defs() -> &'static [ToolDef] {
    static DEFS: [ToolDef; 19] = [
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
            defer: false,
            schema: write::schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "search_replace",
            blurb: "Replace text in a workspace file",
            defer: false,
            schema: search_replace::schema,
            visible: not_readonly,
        },
        ToolDef {
            name: "run_terminal_command",
            blurb: "Run a shell command in the workspace",
            defer: false,
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
            name: show_panel::SHOW_PANEL,
            blurb: "Emit a validated generative UI panel spec",
            defer: true,
            schema: show_panel::schema,
            visible: generative_ui_on,
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
pub fn desktop_tail_defs() -> &'static [ToolDef] {
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

/// Every native row in legacy order.
pub fn all_defs() -> impl Iterator<Item = &'static ToolDef> {
    read_defs()
        .iter()
        .chain(action_defs())
        .chain(desktop_tail_defs())
}

pub fn find_def(name: &str) -> Option<ToolDef> {
    all_defs().find(|d| d.name == name).copied()
}
