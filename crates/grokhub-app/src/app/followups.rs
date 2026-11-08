//! Messages typed while a reply runs wait here, each tied to the chat it was
//! typed in. A message only ever sends into its own chat: one typed in chat B
//! while chat A replies waits for B, even if A is the chat on screen when A's
//! reply ends.

use std::ops::Index;

/// One waiting message and the chat (thread id) it was typed in. An empty
/// `thread` is untagged and fits any chat (nothing is open, or old tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Queued {
    pub thread: String,
    pub text: String,
    /// Its chat's reply failed: it waits for "continue" instead of sending
    /// when that chat is opened.
    pub paused: bool,
}

impl Queued {
    pub(super) fn fits(&self, thread: &str) -> bool {
        self.thread.is_empty() || self.thread == thread
    }
}

/// The line of waiting messages, in the order they were typed, across chats.
/// Activity lists the whole line; indexes are places in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FollowupQueue {
    items: Vec<Queued>,
    /// The chat and line length an open-chat send last tried, so a send that
    /// never starts isn't tried again every frame. Not saved.
    tried: Option<(String, usize)>,
}

impl FollowupQueue {
    pub(super) fn push(&mut self, thread: &str, text: String) {
        self.items.push(Queued {
            thread: thread.to_string(),
            text,
            paused: false,
        });
    }

    /// Back at the front of its chat's line (a send that never started).
    pub(super) fn push_front(&mut self, thread: &str, text: String) {
        self.items.insert(
            0,
            Queued {
                thread: thread.to_string(),
                text,
                paused: false,
            },
        );
    }

    pub(super) fn extend(&mut self, thread: &str, texts: impl IntoIterator<Item = String>) {
        for text in texts {
            self.push(thread, text);
        }
    }

    pub(super) fn len(&self) -> usize {
        self.items.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many wait for `thread`.
    pub(super) fn count_for(&self, thread: &str) -> usize {
        self.items.iter().filter(|q| q.fits(thread)).count()
    }

    pub(super) fn has_for(&self, thread: &str) -> bool {
        self.items.iter().any(|q| q.fits(thread))
    }

    /// A message typed in `thread` itself (not untagged) is ready to send
    /// the moment that chat is open and idle.
    pub(super) fn ready_for(&self, thread: &str) -> bool {
        !thread.is_empty() && self.items.iter().any(|q| q.thread == thread && !q.paused)
    }

    /// `thread`'s reply failed: its messages wait for "continue".
    pub(super) fn pause_for(&mut self, thread: &str) {
        for q in self.items.iter_mut().filter(|q| q.fits(thread)) {
            q.paused = true;
        }
    }

    /// The next message for `thread`, out of the line.
    pub(super) fn take_next_for(&mut self, thread: &str) -> Option<String> {
        let i = self.items.iter().position(|q| q.fits(thread))?;
        Some(self.items.remove(i).text)
    }

    /// Every message for `thread`, in order, out of the line. Other chats'
    /// messages keep their places.
    pub(super) fn take_all_for(&mut self, thread: &str) -> Vec<String> {
        let (mine, rest): (Vec<Queued>, Vec<Queued>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|q| q.fits(thread));
        self.items = rest;
        mine.into_iter().map(|q| q.text).collect()
    }

    pub(super) fn get(&self, i: usize) -> Option<&Queued> {
        self.items.get(i)
    }

    /// The message at place `i`, out of the line.
    pub(super) fn remove(&mut self, i: usize) -> Option<Queued> {
        (i < self.items.len()).then(|| self.items.remove(i))
    }

    pub(super) fn iter(&self) -> std::slice::Iter<'_, Queued> {
        self.items.iter()
    }

    pub(super) fn texts(&self) -> Vec<String> {
        self.items.iter().map(|q| q.text.clone()).collect()
    }

    pub(super) fn contains_text(&self, text: &str) -> bool {
        self.items.iter().any(|q| q.text == text)
    }

    /// True the first time for this chat and line length, false after.
    pub(super) fn first_try_for(&mut self, thread: &str) -> bool {
        let mark = (thread.to_string(), self.items.len());
        if self.tried.as_ref() == Some(&mark) {
            return false;
        }
        self.tried = Some(mark);
        true
    }

    /// A send went out: the next open-chat send may try again.
    pub(super) fn forget_try(&mut self) {
        self.tried = None;
    }
}

impl super::Cabin {
    /// A chat's title by id, for lines that name another chat.
    pub(super) fn thread_title(&self, id: &str) -> Option<String> {
        self.threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.title.clone())
    }

    /// Moves queued follow-ups into the composer, after anything already
    /// typed there. Returns how many moved. Only the open chat's messages
    /// move: another chat's wait for that chat.
    pub(super) fn queued_back_to_composer(&mut self) -> usize {
        let visible = self.visible_thread_id();
        let queued = self.followup_queue.take_all_for(&visible);
        let moved = queued.len();
        if moved > 0 {
            self.composer = super::composer_with_queued(&self.composer, queued);
            self.composer_want_focus = true;
        }
        moved
    }
}

/// Untagged entries (they fit any chat), for tests and old call sites.
impl From<Vec<String>> for FollowupQueue {
    fn from(texts: Vec<String>) -> Self {
        Self {
            items: texts
                .into_iter()
                .map(|text| Queued {
                    thread: String::new(),
                    text,
                    paused: false,
                })
                .collect(),
            tried: None,
        }
    }
}

impl Index<usize> for FollowupQueue {
    type Output = String;
    fn index(&self, i: usize) -> &String {
        &self.items[i].text
    }
}

impl PartialEq<Vec<String>> for FollowupQueue {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.items.len() == other.len() && self.items.iter().zip(other).all(|(q, t)| &q.text == t)
    }
}

impl<const N: usize> PartialEq<[&str; N]> for FollowupQueue {
    fn eq(&self, other: &[&str; N]) -> bool {
        self.items.len() == N && self.items.iter().zip(other).all(|(q, t)| q.text == *t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_chat_takes_only_its_own_messages_in_order() {
        let mut q = FollowupQueue::default();
        q.push("a", "a1".into());
        q.push("b", "b1".into());
        q.push("a", "a2".into());
        assert_eq!(q.count_for("b"), 1);
        assert_eq!(q.take_next_for("b").as_deref(), Some("b1"));
        assert_eq!(q.take_next_for("b"), None);
        assert_eq!(q.take_next_for("a").as_deref(), Some("a1"));
        assert_eq!(q, ["a2"]);
    }

    #[test]
    fn take_all_leaves_other_chats_waiting() {
        let mut q = FollowupQueue::default();
        q.push("a", "a1".into());
        q.push("b", "b1".into());
        q.push("a", "a2".into());
        assert_eq!(
            q.take_all_for("a"),
            vec!["a1".to_string(), "a2".to_string()]
        );
        assert_eq!(q.len(), 1);
        assert_eq!(q.get(0).map(|e| e.thread.as_str()), Some("b"));
    }

    #[test]
    fn untagged_messages_fit_any_chat_and_front_goes_first() {
        let mut q: FollowupQueue = vec!["x".to_string()].into();
        q.push_front("c", "first".into());
        assert!(q.has_for("zzz"));
        assert_eq!(q.take_next_for("c").as_deref(), Some("first"));
        assert_eq!(q.take_next_for("zzz").as_deref(), Some("x"));
        assert!(q.is_empty());
        assert_eq!(q.remove(0), None);
    }

    #[test]
    fn only_a_chats_own_unpaused_messages_are_ready_for_it() {
        let mut q: FollowupQueue = vec!["untagged".to_string()].into();
        assert!(
            !q.ready_for("a"),
            "an untagged message never sends by itself"
        );
        q.push("a", "a1".into());
        assert!(q.ready_for("a"));
        assert!(!q.ready_for("b"));
        assert!(!q.ready_for(""));
        q.pause_for("a");
        assert!(
            !q.ready_for("a"),
            "a failed reply's line waits for continue"
        );
        assert_eq!(q.take_next_for("a").as_deref(), Some("untagged"));
        assert_eq!(q.take_next_for("a").as_deref(), Some("a1"));
    }
}
