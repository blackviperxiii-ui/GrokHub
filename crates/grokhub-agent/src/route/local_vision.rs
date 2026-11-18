//! Pictures in a local chat (local router, step 7).
//!
//! A `llama-server` started without `--mmproj` refuses any request that
//! carries an `image_url` part, and pictures stay in the chat's history, so
//! every later local turn of that chat failed the same way. A server with no
//! picture reader therefore gets each picture swapped for [`BLIND_NOTE`]:
//! the turn is answered without the picture instead of failing.

use crate::client::{ContentPart, InputItem};

use super::local_autoconfig::LaunchArgs;

/// What stands in for a picture the running model can't see.
pub const BLIND_NOTE: &str =
    "[A picture was attached here. The model on this computer can't see pictures, so it wasn't read.]";

/// Whether the running launch can read pictures: it was started with its
/// projector (`--mmproj`). The one-shot defaults never carry one.
pub fn sees_pictures(launch: Option<&LaunchArgs>) -> bool {
    launch.is_some_and(|l| l.mmproj.is_some())
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

    #[test]
    fn only_a_launch_with_its_projector_sees_pictures() {
        assert!(!sees_pictures(None));
        let mut l = LaunchArgs {
            backend: grokhub_core::local_profile::Backend::Vulkan,
            ngl: 40,
            all_layers: 40,
            n_cpu_moe: None,
            ctx: 8192,
            kv: super::super::local_autoconfig::KvType::Q8,
            threads: 4,
            threads_batch: None,
            flash_attn: true,
            mlock: false,
            tensor_split: None,
            draft: None,
            mmproj: None,
            cpu_only: false,
            run: Default::default(),
        };
        assert!(!sees_pictures(Some(&l)));
        assert!(!l.to_args().iter().any(|a| a.contains("mmproj")));
        l.mmproj = Some("/m/p.gguf".into());
        assert!(sees_pictures(Some(&l)));
    }

    /// The projector flags the launch passes, each listed by the pinned
    /// engine's `--help`.
    #[test]
    fn a_launch_with_its_projector_passes_mmproj() {
        let gpu = LaunchArgs {
            backend: grokhub_core::local_profile::Backend::Vulkan,
            ngl: 40,
            all_layers: 40,
            n_cpu_moe: None,
            ctx: 8192,
            kv: super::super::local_autoconfig::KvType::Q8,
            threads: 4,
            threads_batch: None,
            flash_attn: true,
            mlock: false,
            tensor_split: None,
            draft: None,
            mmproj: Some("/m/p.gguf".into()),
            cpu_only: false,
            run: Default::default(),
        };
        let mmproj = |l: &LaunchArgs| -> Vec<String> {
            let a = l.to_args();
            let at = a.iter().position(|x| x == "--mmproj").unwrap();
            a[at..]
                .iter()
                .take_while(|x| *x != "--device" && *x != "--cache-prompt")
                .cloned()
                .collect()
        };
        assert_eq!(mmproj(&gpu), ["--mmproj", "/m/p.gguf"]);
        // No layers on the GPU: the projector stays off it too.
        let no_layers = LaunchArgs {
            ngl: 0,
            ..gpu.clone()
        };
        assert_eq!(
            mmproj(&no_layers),
            ["--mmproj", "/m/p.gguf", "--no-mmproj-offload"]
        );
        // `--device none` already keeps it off every GPU.
        let cpu = super::super::local_autoconfig::on_cpu(&gpu);
        assert_eq!(mmproj(&cpu), ["--mmproj", "/m/p.gguf"]);
        assert_eq!(
            cpu.to_args().last().map(String::as_str),
            Some("--cache-prompt")
        );
        let help =
            include_str!("../../../grokhub-core/tests/fixtures/llama_server_help_b12017.txt");
        assert!(help.contains("--mmproj FILE"));
        assert!(help.contains("--no-mmproj-offload"));
    }
}
