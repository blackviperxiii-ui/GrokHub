use super::*;
use serde_json::json;

fn routine() -> Routine {
    Routine::parse(
        &json!({
            "version": 1, "name": "resize", "about": "Shrink a picture.",
            "inputs": {
                "file": { "type": "string", "about": "the picture" },
                "width": { "type": "integer" },
                "keep": { "type": "boolean", "default": true }
            },
            "steps": [{ "id": "a", "tool": "read_file", "args": { "target_file": "{{inputs.file}}" } }]
        })
        .to_string(),
    )
    .unwrap()
}

#[test]
fn every_call_is_a_low_effort_schema_call_with_no_tools() {
    let req = request("grok-4.3", "routine-resize", REPAIR_FORMAT, "sys", "user");
    assert_eq!(req.text_format, Some(REPAIR_FORMAT));
    assert_eq!(req.effort.as_deref(), Some("low"));
    assert!(req.tools.is_empty() && !req.hosted_search);
    assert_eq!(req.call_timeout, Some(Duration::from_secs(30)));
    for f in [FILL_FORMAT, BRANCH_FORMAT, REPAIR_FORMAT] {
        assert_eq!(f.wire().expect("schema parses")["format"]["strict"], true);
    }
}

#[test]
fn a_fill_types_each_value_and_says_what_is_still_missing() {
    let r = routine();
    let missing = ["file".to_string(), "width".to_string()];
    assert_eq!(
        parse_fill(
            r#"{"values":[{"name":"file","value":" cat.png "},{"name":"width","value":"640"},{"name":"keep","value":"false"}]}"#,
            &r,
            &missing
        ),
        Ok(json!({ "file": "cat.png", "width": 640 })
            .as_object()
            .unwrap()
            .clone())
    );
    assert_eq!(
        parse_fill(
            r#"{"values":[{"name":"file","value":"cat.png"}]}"#,
            &r,
            &missing
        ),
        Err("it needs a value for inputs.width".into())
    );
    assert_eq!(
        parse_fill(
            r#"{"values":[{"name":"file","value":"a"},{"name":"width","value":"wide"}]}"#,
            &r,
            &missing
        ),
        Err("the model gave \"wide\" for inputs.width, which isn't a integer".into())
    );
    assert_eq!(
        parse_fill("sorry", &r, &missing),
        Err("the model's answer about the inputs didn't parse".into())
    );
}

#[test]
fn a_branch_must_be_one_of_the_names() {
    assert_eq!(
        parse_branch(
            r#"{"branch":"photo","why":"It's a JPEG."}"#,
            &["document", "photo"]
        ),
        Ok(("photo".into(), "It's a JPEG.".into()))
    );
    assert_eq!(
        parse_branch(r#"{"branch":"video","why":""}"#, &["document", "photo"]),
        Err("the model chose \"video\", which isn't one of document, photo".into())
    );
}

#[test]
fn a_repair_is_retry_with_new_arguments_skip_stop_or_replan() {
    assert_eq!(
        parse_repair(
            r#"{"do":"retry","args_json":"{\"command\":\"df -P /home\"}","why":"Use /home."}"#
        ),
        Ok((
            Repair::Retry(
                json!({ "command": "df -P /home" })
                    .as_object()
                    .unwrap()
                    .clone()
            ),
            "Use /home.".into()
        ))
    );
    assert_eq!(
        parse_repair(r#"{"do":"skip","args_json":"{}","why":"Not needed."}"#),
        Ok((Repair::Skip, "Not needed.".into()))
    );
    assert_eq!(
        parse_repair(r#"{"do":"replan","args_json":"{}","why":"No such disk."}"#),
        Ok((Repair::Replan, "No such disk.".into()))
    );
    assert_eq!(
        parse_repair(r#"{"do":"retry","args_json":"{}","why":"Again."}"#),
        Err("the model said retry but gave no new arguments".into())
    );
    assert_eq!(
        parse_repair(r#"{"do":"delete","args_json":"{}","why":""}"#),
        Err("the model's fix \"delete\" isn't one this run can do".into())
    );
}

/// Answers with the replies it was given, in order, and keeps the asks.
struct Canned(
    std::cell::RefCell<Vec<(&'static str, &'static str, String)>>,
    std::cell::RefCell<Vec<String>>,
);

impl Brain for Canned {
    fn ask(
        &self,
        class: &'static str,
        format: TextFormat,
        _system: &str,
        user: &str,
    ) -> Option<String> {
        self.0
            .borrow_mut()
            .push((class, format.name, user.to_string()));
        let mut replies = self.1.borrow_mut();
        (!replies.is_empty()).then(|| replies.remove(0))
    }
}

#[test]
fn the_model_reads_the_words_as_data_and_never_sees_more_than_it_needs() {
    let brain = Canned(
        Default::default(),
        std::cell::RefCell::new(vec![
            r#"{"values":[{"name":"file","value":"cat.png"},{"name":"width","value":"640"}]}"#
                .into(),
        ]),
    );
    let r = routine();
    let got = fill(
        &brain,
        &r,
        &["file".into(), "width".into()],
        "shrink cat.png to 640 wide",
    );
    assert_eq!(
        got,
        Ok(json!({ "file": "cat.png", "width": 640 })
            .as_object()
            .unwrap()
            .clone())
    );
    let asks = brain.0.borrow();
    assert_eq!((asks[0].0, asks[0].1), (FILL_CLASS, "routine_fill"));
    assert_eq!(
        asks[0].2,
        "Routine: resize\nShrink a picture.\n\nMissing inputs:\n- file (string): the picture\n- width (integer)\n\nThe person's words:\n```\nshrink cat.png to 640 wide\n```"
    );
    drop(asks);
    // No answer at all: nothing is guessed.
    assert_eq!(
        fill(&brain, &r, &["file".into()], "anything"),
        Err("the model couldn't be reached to fill in the inputs".into())
    );
}
