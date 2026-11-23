//! First-class Home Assistant tools. Offered only when the app registers a
//! host (consent + URL + token). Read tools list and inspect devices; the
//! control tool calls a service and goes through the normal ask gate.

use std::sync::{Arc, RwLock};

use serde_json::{json, Map, Value};

use super::{str_field, ToolOutput};

pub const LIST_ENTITIES: &str = "home_list_entities";
pub const GET_STATE: &str = "home_get_state";
pub const LIST_SERVICES: &str = "home_list_services";
pub const CALL_SERVICE: &str = "home_call_service";

/// The app side: already checked consent and credentials.
pub trait HomeAssistantOps: Send + Sync {
    fn list_entities(&self, domain: Option<&str>, area: Option<&str>) -> Result<String, String>;
    fn get_state(&self, entity_id: &str) -> Result<String, String>;
    fn list_services(&self, domain: Option<&str>) -> Result<String, String>;
    fn call_service(
        &self,
        domain: &str,
        service: &str,
        entity_id: Option<&str>,
        data: Option<&Map<String, Value>>,
    ) -> Result<String, String>;
}

static HOST: RwLock<Option<Arc<dyn HomeAssistantOps>>> = RwLock::new(None);

#[cfg(test)]
thread_local! {
    static TEST_HOST: std::cell::RefCell<Option<Arc<dyn HomeAssistantOps>>> =
        const { std::cell::RefCell::new(None) };
}

pub fn set_host(host: Arc<dyn HomeAssistantOps>) {
    *HOST.write().unwrap_or_else(|e| e.into_inner()) = Some(host);
}

pub fn clear_host() {
    *HOST.write().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
pub(crate) struct TestHost;

#[cfg(test)]
impl TestHost {
    pub(crate) fn set(host: Arc<dyn HomeAssistantOps>) -> Self {
        TEST_HOST.with(|h| *h.borrow_mut() = Some(host));
        Self
    }
}

#[cfg(test)]
impl Drop for TestHost {
    fn drop(&mut self) {
        TEST_HOST.with(|h| *h.borrow_mut() = None);
    }
}

fn host() -> Option<Arc<dyn HomeAssistantOps>> {
    #[cfg(test)]
    {
        if let Some(h) = TEST_HOST.with(|h| h.borrow().clone()) {
            return Some(h);
        }
        None
    }
    #[cfg(not(test))]
    {
        HOST.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Tools are offered only where the app hosts them with consent.
pub fn available() -> bool {
    host().is_some()
}

pub const OFF_MSG: &str =
    "Home Assistant is off. Turn it on under Settings → Your PC → PC helpers, add the address and a long-lived access token, then try again.";

fn need_host() -> Result<Arc<dyn HomeAssistantOps>, ToolOutput> {
    host().ok_or_else(|| ToolOutput::err(OFF_MSG))
}

pub fn list_entities_schema() -> Value {
    json!({
        "name": LIST_ENTITIES,
        "description": "List Home Assistant entities (local devices), optionally filtered by domain or area/name.",
        "parameters": {
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Optional domain filter, e.g. light, switch, climate, sensor."
                },
                "area": {
                    "type": "string",
                    "description": "Optional room or friendly-name needle, e.g. kitchen."
                }
            },
            "additionalProperties": false
        }
    })
}

pub fn get_state_schema() -> Value {
    json!({
        "name": GET_STATE,
        "description": "Read one Home Assistant entity's state and attributes.",
        "parameters": {
            "type": "object",
            "properties": {
                "entity_id": {
                    "type": "string",
                    "description": "Entity id, e.g. light.living_room."
                }
            },
            "required": ["entity_id"],
            "additionalProperties": false
        }
    })
}

pub fn list_services_schema() -> Value {
    json!({
        "name": LIST_SERVICES,
        "description": "List Home Assistant services (actions) available for local device control.",
        "parameters": {
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Optional domain filter, e.g. light or climate."
                }
            },
            "additionalProperties": false
        }
    })
}

pub fn call_service_schema() -> Value {
    json!({
        "name": CALL_SERVICE,
        "description": "Call a Home Assistant service to control a local device. Dangerous host/script domains are blocked.",
        "parameters": {
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Service domain, e.g. light, switch, climate."
                },
                "service": {
                    "type": "string",
                    "description": "Service name, e.g. turn_on, turn_off, set_temperature."
                },
                "entity_id": {
                    "type": "string",
                    "description": "Optional target entity id."
                },
                "data": {
                    "type": "object",
                    "description": "Optional extra service data (brightness, temperature, …)."
                }
            },
            "required": ["domain", "service"],
            "additionalProperties": false
        }
    })
}

pub fn run(name: &str, args: &Value) -> ToolOutput {
    let host = match need_host() {
        Ok(h) => h,
        Err(e) => return e,
    };
    let result = match name {
        LIST_ENTITIES => {
            let domain = opt_str(args, "domain");
            let area = opt_str(args, "area");
            host.list_entities(domain.as_deref(), area.as_deref())
        }
        GET_STATE => {
            let id = str_field(args, "entity_id");
            if id.is_empty() {
                return ToolOutput::err("entity_id is required.");
            }
            host.get_state(&id)
        }
        LIST_SERVICES => {
            let domain = opt_str(args, "domain");
            host.list_services(domain.as_deref())
        }
        CALL_SERVICE => {
            let domain = str_field(args, "domain");
            let service = str_field(args, "service");
            if domain.is_empty() || service.is_empty() {
                return ToolOutput::err("domain and service are required.");
            }
            let entity_id = opt_str(args, "entity_id");
            let data = args.get("data").and_then(Value::as_object);
            host.call_service(&domain, &service, entity_id.as_deref(), data)
        }
        other => return ToolOutput::err(format!("Unknown Home Assistant tool `{other}`.")),
    };
    match result {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    let s = str_field(args, key);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        calls: Mutex<Vec<String>>,
    }

    impl HomeAssistantOps for Fake {
        fn list_entities(
            &self,
            domain: Option<&str>,
            area: Option<&str>,
        ) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("list:{domain:?}:{area:?}"));
            Ok("1 entities:\n- light.a [on] A".into())
        }
        fn get_state(&self, entity_id: &str) -> Result<String, String> {
            self.calls.lock().unwrap().push(format!("get:{entity_id}"));
            Ok(format!(
                "entity_id: {entity_id}\nstate: on\nattributes:\n{{}}"
            ))
        }
        fn list_services(&self, domain: Option<&str>) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("services:{domain:?}"));
            Ok("1 domains:\n- light: turn_on".into())
        }
        fn call_service(
            &self,
            domain: &str,
            service: &str,
            entity_id: Option<&str>,
            _data: Option<&Map<String, Value>>,
        ) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("call:{domain}.{service}:{entity_id:?}"));
            Ok("ok".into())
        }
    }

    #[test]
    fn without_a_host_tools_are_off() {
        assert!(!available());
        let out = run(LIST_ENTITIES, &json!({}));
        assert!(out.failed);
        assert!(out.text.contains("Home Assistant is off"));
    }

    #[test]
    fn with_a_host_list_and_call_reach_it() {
        let fake = Arc::new(Fake {
            calls: Mutex::new(Vec::new()),
        });
        let _g = TestHost::set(fake.clone());
        assert!(available());
        let listed = run(LIST_ENTITIES, &json!({"domain":"light"}));
        assert!(!listed.failed);
        assert!(listed.text.contains("light.a"));
        let called = run(
            CALL_SERVICE,
            &json!({"domain":"light","service":"turn_on","entity_id":"light.a"}),
        );
        assert!(!called.failed);
        let calls = fake.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                "list:Some(\"light\"):None".to_string(),
                "call:light.turn_on:Some(\"light.a\")".to_string()
            ]
        );
    }
}
