//! Pictures in a local chat (local router, step 7, first part).
//!
//! A `llama-server` started without `--mmproj` refuses any request that
//! carries an `image_url` part, and pictures stay in the chat's history, so
//! every later local turn of that chat failed the same way. No chat launch
//! carries a picture reader yet, so each picture is swapped for
//! [`BLIND_NOTE`]: the turn is answered without the picture instead of
//! failing.

use std::collections::BTreeMap;

use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::Registry;

use crate::client::{ContentPart, InputItem};

use super::RouteInput;

/// What stands in for a picture the running model can't see.
pub const BLIND_NOTE: &str =
    "[A picture was attached here. The model on this computer can't see pictures, so it wasn't read.]";

/// The router's rule when [`reads_online`] took the device out of the pick.
pub const NO_VISION_RULE: &str = "local:no_vision";

/// A picture the device can't see goes to an online model that can, when
/// the turn may leave this computer: never an 18+ turn, a voice set to
/// Local only, sensitive data without your grant, or offline. Online, only
/// background steps reach the device through the router (a chat is never in
/// [`super::local::LOCAL_CLASSES`]), so a chat picture already goes online
/// by the image fit in [`super::policy::fits`].
pub fn reads_online(input: &RouteInput<'_>) -> bool {
    input.needs_image
        && !input.adult
        && !matches!(
            input.voice_reply,
            Some(super::voice_route::ReplyPref::LocalOnly)
        )
        && !(input.local.sensitive && !input.local.cloud_grant)
        && !input.local.offline
        && !input.local.offline_switch
}

/// `id` is listed as reading pictures. A model whose input kinds are
/// unknown may not, so it never stands in for the device on a picture:
/// the chat would say an online model read a picture nothing read.
pub fn reads_pictures(reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, id: &str) -> bool {
    profiles
        .get(id)
        .and_then(|p| p.metadata.input_modalities.as_ref())
        .or(reg.get(id).and_then(|r| r.meta.input_modalities.as_ref()))
        .is_some_and(|m| m.iter().any(|x| x == "image"))
}

/// Turn every picture in `input` into [`BLIND_NOTE`]. Returns how many it
/// replaced.
pub fn strip_images(input: &mut [InputItem]) -> usize {
    let mut n = 0;
    for item in input {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content.iter_mut() {
            if matches!(part, ContentPart::InputImage(_)) {
                *part = ContentPart::InputText(BLIND_NOTE.into());
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ResponsesRequest;

    fn user(parts: Vec<ContentPart>) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: parts,
        }
    }

    fn img() -> ContentPart {
        ContentPart::InputImage("data:image/png;base64,YQ==".into())
    }

    #[test]
    fn every_picture_becomes_the_note_and_text_stays() {
        let mut input = vec![
            user(vec![ContentPart::InputText("what is this?".into()), img()]),
            InputItem::FunctionCallOutput {
                call_id: "c1".into(),
                output: "done".into(),
            },
            user(vec![
                ContentPart::InputText("image from screenshot".into()),
                img(),
            ]),
        ];
        assert_eq!(strip_images(&mut input), 2);
        assert_eq!(
            input[0],
            user(vec![
                ContentPart::InputText("what is this?".into()),
                ContentPart::InputText(BLIND_NOTE.into()),
            ])
        );
        assert_eq!(
            input[2],
            user(vec![
                ContentPart::InputText("image from screenshot".into()),
                ContentPart::InputText(BLIND_NOTE.into()),
            ])
        );
        assert_eq!(strip_images(&mut input), 0);
    }

    #[test]
    fn a_stripped_request_posts_no_image_url() {
        let mut req = ResponsesRequest {
            model: "local".into(),
            effort: None,
            input: vec![user(vec![ContentPart::InputText("hi".into()), img()])],
            conversation_id: "c".into(),
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
            text_format: None,
        };
        strip_images(&mut req.input);
        let body = super::super::providers::openai_body("local", None, &req);
        assert_eq!(
            body["messages"][0]["content"],
            serde_json::json!(format!("hi\n{BLIND_NOTE}"))
        );
        assert!(!body.to_string().contains("image_url"));
    }
}
