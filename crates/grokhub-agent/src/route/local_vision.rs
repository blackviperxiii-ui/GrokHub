//! Pictures in a local chat (local router, step 7).
//!
//! A `llama-server` started without `--mmproj` refuses any request that
//! carries an `image_url` part, and pictures stay in the chat's history, so
//! every later local turn of that chat failed the same way. A launch that
//! carries its vision projector reads them; for any other, each picture is
//! swapped for [`BLIND_NOTE`]: the turn is answered without the picture
//! instead of failing.
//!
//! The projector is the catalog's `projector` row for the chat model, its
//! file beside the model's. It loads only for a chat whose newest message
//! carries a picture ([`PictureGuard`]); an engine that refused it with a
//! model keeps a lasting record ([`note_refused`]), so it isn't tried again
//! until the engine changes.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
/// by the image fit in [`super::policy::fits`]. A step rides on the model
/// loaded now (background work never swaps one), so the device keeps a
/// picture when that launch carries its projector
/// ([`super::local::LocalGate::sees_pictures`]).
pub fn reads_online(input: &RouteInput<'_>) -> bool {
    input.needs_image
        && !input.local.sees_pictures
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

/// Where this thread's request stands with a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picture {
    None,
    /// The person's newest message carries one.
    Wanted,
    /// ...and the turn already waited for the requests on a server that
    /// can't see it ([`note_waited`]).
    Waited,
}

thread_local! {
    static PICTURE: Cell<Picture> = const { Cell::new(Picture::None) };
}

/// Marks this thread's request as carrying a picture in the person's newest
/// message, for the swap planner, until dropped.
pub struct PictureGuard(Picture);

impl PictureGuard {
    pub fn set(on: bool) -> Self {
        let p = if on { Picture::Wanted } else { Picture::None };
        PictureGuard(PICTURE.with(|c| c.replace(p)))
    }
}

impl Drop for PictureGuard {
    fn drop(&mut self) {
        PICTURE.with(|c| c.set(self.0));
    }
}

/// This thread's request wants its newest picture read ([`PictureGuard`]).
pub fn wants_picture() -> bool {
    PICTURE.with(Cell::get) != Picture::None
}

/// The picture turn waited, as long as it may, for the requests on the
/// running server (background helpers hold it right after a person's
/// message): the swap goes ahead past them now.
pub fn note_waited() {
    PICTURE.with(|c| {
        if c.get() == Picture::Wanted {
            c.set(Picture::Waited);
        }
    });
}

/// [`note_waited`] ran for this thread's picture turn.
pub fn waited() -> bool {
    PICTURE.with(Cell::get) == Picture::Waited
}

/// The catalog's vision projector for the chat model in `model`, when its
/// file sits beside the model's (projectors download to the models folder).
pub fn projector_file(model: &Path) -> Option<PathBuf> {
    let row = super::local_autoconfig::catalog_model_for(model)?;
    let v = grokhub_core::local_setup::model_by_id(row.projector?)?;
    let path = model.with_file_name(v.file);
    path.is_file().then_some(path)
}

/// Where the lasting record of projectors an engine refused lives, in the
/// folder beside `autotune.json`.
pub const REFUSED_FILE: &str = "vision_refused.json";

/// The record's key: the model file and the engine build (its size), so a
/// new engine tries again.
fn refused_key(binary: &Path, model: &Path) -> Option<String> {
    let engine = std::fs::metadata(binary).ok()?.len();
    let file = model.file_name()?.to_str()?;
    Some(format!("{file}|engine:{engine}"))
}

fn read_refused(dir: &Path) -> BTreeMap<String, bool> {
    std::fs::read_to_string(dir.join(REFUSED_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The engine in `binary` refused `model`'s projector (see [`note_refused`]).
pub fn refused(binary: &Path, model: &Path) -> bool {
    super::local_autoconfig::tune_dir().is_some_and(|d| refused_in(&d, binary, model))
}

pub fn refused_in(dir: &Path, binary: &Path, model: &Path) -> bool {
    refused_key(binary, model).is_some_and(|k| read_refused(dir).contains_key(&k))
}

/// A start with `model`'s projector failed and the same launch without it
/// ran: this engine won't load the projector with that model (beside its
/// drafter, when it has one). Vision skips the model until the engine
/// changes.
pub fn note_refused(binary: &Path, model: &Path) {
    if let Some(dir) = super::local_autoconfig::tune_dir() {
        let _ = note_refused_in(&dir, binary, model);
    }
}

pub fn note_refused_in(dir: &Path, binary: &Path, model: &Path) -> Result<(), String> {
    let key = refused_key(binary, model).ok_or("the engine or model file is missing")?;
    let mut all = read_refused(dir);
    all.insert(key, true);
    let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(REFUSED_FILE), format!("{text}\n")).map_err(|e| e.to_string())
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
