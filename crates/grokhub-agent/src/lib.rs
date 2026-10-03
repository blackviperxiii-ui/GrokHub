//! Read-only native engine. Sync HTTP, no CLI, no credential files.

mod client;
mod events;
mod models;
mod prompt;
mod retry;
mod run;
mod scan;
mod sse;
mod tools;

pub use client::{
    map_http_status, request_headers, responses_body, AuthKind, CancelToken, ClientError,
    ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput,
    Usage, XaiClient, DEFAULT_MODEL, RESPONSES_URL, USER_AGENT,
};
pub use events::{meter_for, Engine, EngineParts, NativeEngine, StampHalt};
pub use models::{parse_xai_models, pick_model, XAI_MODELS_URL};
pub use prompt::system_prompt;
pub use retry::{
    decide_retry, jitter_backoff, resolve_max_retries_with_env, retry_after_or_backoff,
    retry_backoff_with_jitter, DEFAULT_MAX_RETRIES, MAX_RETRY_BACKOFF, RATE_LIMIT_RETRY_THRESHOLD,
    TRANSPORT_REBUILD_BACKOFF,
};
pub use run::{
    run_loop, HaltCheck, LoopEvent, LoopIn, LoopOut, SteerQueue, StopReason, DEFAULT_MAX_TURNS,
};
pub use sse::SseParser;
pub use tools::{execute, is_readonly, tool_schemas, ToolOutput, READ_ONLY_PHASE};
