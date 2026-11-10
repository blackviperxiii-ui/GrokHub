//! The park handoff and elicit card answered in memory, for tests in this
//! crate and in the apps that drive [`super::SelfServer`].

use serde_json::{json, Value};

use super::{SelfIo, SelfServer};
use crate::harness as hx;

/// The park handoff and elicit card, answered by the test.
pub struct FakeIo {
    pub approve: bool,
    pub parks: Vec<hx::ParkRequest>,
    pub secret: Option<String>,
    pub elicits: Vec<Value>,
}

impl FakeIo {
    pub fn answering(approve: bool) -> Self {
        Self {
            approve,
            parks: Vec::new(),
            secret: None,
            elicits: Vec::new(),
        }
    }
}

impl SelfIo for FakeIo {
    fn park(&mut self, req: &hx::ParkRequest) -> bool {
        self.parks.push(req.clone());
        self.approve
    }

    fn elicit(&mut self, params: Value) -> Option<Value> {
        let var = params
            .pointer("/requestedSchema/required/0")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.elicits.push(params);
        Some(match &self.secret {
            Some(v) => json!({ "action": "accept", "content": { var: v } }),
            None => json!({ "action": "decline" }),
        })
    }

    fn halted(&mut self) -> bool {
        false
    }
}

/// One `tools/call` through the server: `(ok, text)`.
#[allow(clippy::unwrap_used)]
pub fn call(
    server: &mut SelfServer<'_>,
    io: &mut FakeIo,
    tool: &str,
    args: Value,
) -> (bool, String) {
    let line = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": tool, "arguments": args } });
    let reply: Value =
        serde_json::from_str(&server.handle_line(&line.to_string(), io).unwrap()).unwrap();
    let r = &reply["result"];
    (
        !r["isError"].as_bool().unwrap(),
        r["content"][0]["text"].as_str().unwrap().to_string(),
    )
}
