// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.
//! Image byte budget. Evicts the oldest `input_image` parts once a request body
//! crosses the proxy ceiling, using the CLI placeholder so the model does not
//! invent the picture from memory.

use crate::client::{input_wire_len, ContentPart, InputItem};

/// Replaces an inline image evicted to keep the request body under the proxy's size limit.
pub const IMAGE_COMPACT_PLACEHOLDER: &str = "[An earlier image was removed to keep the request within its size limit and is no longer visible. Do not describe or reason about its contents from memory; ask the user to re-share it if you need to see it again.]";

/// Hard request-body ceiling. Inline image data dominates the body.
pub const MAX_REQUEST_BYTES: usize = 50 * 1024 * 1024;

/// Evict once the serialized input reaches this size (3 MB below the ceiling).
pub const IMAGE_COMPACT_TRIGGER_BYTES: usize = MAX_REQUEST_BYTES - 3 * 1024 * 1024;

/// Low-water mark eviction reclaims down to, so the next turns stay under the trigger.
pub const IMAGE_COMPACT_RECLAIM_TARGET_BYTES: usize = MAX_REQUEST_BYTES / 2;

const _: () = assert!(IMAGE_COMPACT_RECLAIM_TARGET_BYTES < IMAGE_COMPACT_TRIGGER_BYTES);

/// Apply the default proxy budget. Returns how many images were replaced.
pub fn apply_image_budget(items: &mut [InputItem]) -> usize {
    apply_image_budget_with_limits(
        items,
        IMAGE_COMPACT_TRIGGER_BYTES,
        IMAGE_COMPACT_RECLAIM_TARGET_BYTES,
    )
}

/// Evict oldest images first until the wire size is at or under `reclaim_bytes`.
/// No image changes when the body is under `trigger_bytes`.
pub fn apply_image_budget_with_limits(
    items: &mut [InputItem],
    trigger_bytes: usize,
    reclaim_bytes: usize,
) -> usize {
    if input_wire_len(items) < trigger_bytes {
        return 0;
    }
    let mut evicted = 0usize;
    while input_wire_len(items) > reclaim_bytes {
        if !evict_oldest_image(items) {
            break;
        }
        evicted = evicted.saturating_add(1);
    }
    evicted
}

fn evict_oldest_image(items: &mut [InputItem]) -> bool {
    for item in items.iter_mut() {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content.iter_mut() {
            if matches!(part, ContentPart::InputImage(_)) {
                *part = ContentPart::InputText(IMAGE_COMPACT_PLACEHOLDER.to_string());
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_message(url: &str) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText("see".into()),
                ContentPart::InputImage(url.into()),
            ],
        }
    }

    fn urls(items: &[InputItem]) -> Vec<Option<String>> {
        let mut found = Vec::new();
        for item in items {
            let InputItem::Message { content, .. } = item else {
                continue;
            };
            for part in content {
                match part {
                    ContentPart::InputImage(url) => found.push(Some(url.clone())),
                    ContentPart::InputText(text) if text == IMAGE_COMPACT_PLACEHOLDER => {
                        found.push(None);
                    }
                    ContentPart::InputText(_) => {}
                }
            }
        }
        found
    }

    #[test]
    fn evicts_oldest_images_first_with_the_cli_placeholder() {
        let oldest = format!("data:image/png;base64,{}", "A".repeat(800));
        let middle = format!("data:image/png;base64,{}", "B".repeat(800));
        let newest = format!("data:image/png;base64,{}", "C".repeat(400));
        let mut items = vec![
            image_message(&oldest),
            image_message(&middle),
            image_message(&newest),
        ];
        let full = input_wire_len(&items);
        let mut after_two = items.clone();
        assert!(evict_oldest_image(&mut after_two));
        assert!(evict_oldest_image(&mut after_two));
        let reclaim = input_wire_len(&after_two);
        assert!(reclaim < full);
        let evicted = apply_image_budget_with_limits(&mut items, full, reclaim);
        assert_eq!(
            evicted,
            2,
            "wire {} -> target {reclaim}",
            input_wire_len(&items)
        );
        assert_eq!(
            urls(&items),
            vec![None, None, Some(newest)],
            "oldest images go first and the placeholder is the CLI text"
        );
        assert!(input_wire_len(&items) <= reclaim);
    }

    #[test]
    fn images_inside_one_message_evict_left_to_right() {
        let first = format!("data:image/png;base64,{}", "F".repeat(4_000));
        let second = format!("data:image/png;base64,{}", "S".repeat(4_000));
        let mut items = vec![InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputImage(first.clone()),
                ContentPart::InputText("between".into()),
                ContentPart::InputImage(second.clone()),
            ],
        }];
        let full = input_wire_len(&items);
        assert!(evict_oldest_image(&mut items));
        let after = input_wire_len(&items);
        assert!(after < full, "evicting a large image must shrink the body");
        let mut again = vec![InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputImage(first),
                ContentPart::InputText("between".into()),
                ContentPart::InputImage(second.clone()),
            ],
        }];
        let evicted = apply_image_budget_with_limits(&mut again, full, after);
        assert_eq!(evicted, 1);
        let InputItem::Message { content, .. } = &again[0] else {
            panic!("message");
        };
        assert_eq!(
            content[0],
            ContentPart::InputText(IMAGE_COMPACT_PLACEHOLDER.into())
        );
        assert_eq!(content[2], ContentPart::InputImage(second));
    }

    #[test]
    fn under_the_trigger_keeps_every_image() {
        let mut items = vec![image_message("data:image/png;base64,QQ==")];
        let evicted = apply_image_budget_with_limits(&mut items, usize::MAX, 0);
        assert_eq!(evicted, 0);
        assert!(matches!(
            items[0],
            InputItem::Message { ref content, .. }
                if matches!(content.get(1), Some(ContentPart::InputImage(_)))
        ));
    }
}
