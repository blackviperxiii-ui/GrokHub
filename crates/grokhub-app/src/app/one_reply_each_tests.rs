//! A casual chat answers every message in the chat, once and in order: a
//! message typed while a reply runs, messages queued behind a stop, quick
//! short lines and a side question whose side run failed each end with
//! exactly one assistant reply of their own. None is dropped, answered only
//! off to the side, or joined to another.
//!
//! Every send parks its kick behind a pending verify, so no model is called;
//! [`reply`] ends the running turn the way the engine's Done does.

use super::native_engine::set_local_ready_for_test;
use super::*;

struct CasualChat {
    cabin: Cabin,
    root: std::path::PathBuf,
    _verify: mpsc::Sender<Option<VerifyResult>>,
    _hold: std::sync::MutexGuard<'static, ()>,
}

impl Drop for CasualChat {
    fn drop(&mut self) {
        set_local_ready_for_test(None);
        let _ = std::fs::remove_dir_all(&self.root);
        unsafe { std::env::remove_var("GROKHUB_CONFIG") };
    }
}

fn casual_chat(label: &str) -> CasualChat {
    let hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    set_local_ready_for_test(Some(true));
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    let (tx, rx) = mpsc::channel();
    cabin.verify_rx = Some(rx);
    CasualChat {
        cabin,
        root,
        _verify: tx,
        _hold: hold,
    }
}

/// The running turn ends with `text`, as the engine's Done does, and the
/// next message in line goes out.
fn reply(cabin: &mut Cabin, text: &str) {
    assert!(cabin.running, "a reply needs a running turn");
    cabin.pending_kick = None;
    cabin.finish_acp_turn(text.into());
    cabin.drain_followup_queue();
}

fn timeline(cabin: &Cabin) -> Vec<(String, String)> {
    cabin.messages.iter().cloned().collect()
}

/// `typed` in order, each followed by exactly one reply.
fn one_reply_each(typed: &[&str], replies: &[&str]) -> Vec<(String, String)> {
    typed
        .iter()
        .zip(replies)
        .flat_map(|(u, a)| {
            [
                ("user".to_string(), u.to_string()),
                ("assistant".to_string(), a.to_string()),
            ]
        })
        .collect()
}

const FIRST: &str = "ugh rough day, my car broke down on the way home";

#[test]
fn a_question_typed_while_a_casual_reply_runs_gets_its_own_reply_in_the_chat() {
    let mut chat = casual_chat("one-reply-mid-turn");
    let cabin = &mut chat.cabin;
    let q = "How's your day going, other than all that?";
    cabin.send_chat(FIRST.into());
    assert!(cabin.running);
    cabin.send_chat(q.into());
    assert!(
        cabin.side.runs.is_empty(),
        "not answered only on the side card"
    );
    assert_eq!(cabin.followup_queue, [q]);
    reply(cabin, "Oof, that sounds rough.");
    assert!(cabin.running, "the question went out as its own turn");
    reply(cabin, "Pretty chill, thanks for asking!");
    assert_eq!(
        timeline(cabin),
        one_reply_each(
            &[FIRST, q],
            &[
                "Oof, that sounds rough.",
                "Pretty chill, thanks for asking!"
            ]
        )
    );
    assert!(!cabin.running);
}

#[test]
fn two_quick_short_lines_each_get_their_own_reply() {
    let mut chat = casual_chat("one-reply-two-lines");
    let cabin = &mut chat.cabin;
    cabin.send_chat(FIRST.into());
    cabin.send_chat("lol".into());
    cabin.send_chat("yeah same".into());
    assert_eq!(cabin.followup_queue, ["lol", "yeah same"]);
    reply(cabin, "a1");
    reply(cabin, "a2");
    reply(cabin, "a3");
    assert_eq!(
        timeline(cabin),
        one_reply_each(&[FIRST, "lol", "yeah same"], &["a1", "a2", "a3"])
    );
    assert!(cabin.followup_queue.is_empty());
}

#[test]
fn a_stop_hands_queued_lines_back_one_by_one_never_joined() {
    let mut chat = casual_chat("one-reply-stop");
    let cabin = &mut chat.cabin;
    cabin.send_chat(FIRST.into());
    cabin.send_chat("lol".into());
    cabin.send_chat("yeah same".into());
    cabin.send_chat("stop".into());
    assert!(!cabin.running);
    assert_eq!(cabin.composer, "lol", "the first one, word for word");
    assert_eq!(
        cabin.followup_queue,
        ["yeah same"],
        "the other keeps its place"
    );
    assert!(
        cabin.status.ends_with("1 more wait in line"),
        "{}",
        cabin.status
    );
    // Nothing sends by itself right after the stop.
    cabin.send_waiting_here();
    assert!(!cabin.running);
    assert_eq!(cabin.followup_queue, ["yeah same"]);
    let stopped = timeline(cabin);
    let boxed = std::mem::take(&mut cabin.composer);
    cabin.send_chat(boxed);
    reply(cabin, "a2");
    reply(cabin, "a3");
    let after = timeline(cabin)[stopped.len()..].to_vec();
    assert_eq!(after, one_reply_each(&["lol", "yeah same"], &["a2", "a3"]));
}

#[test]
fn a_side_question_whose_side_run_fails_is_answered_in_the_chat() {
    let mut chat = casual_chat("one-reply-side-fail");
    let cabin = &mut chat.cabin;
    cabin.send_chat(FIRST.into());
    cabin.send_chat("/btw what time is it".into());
    assert_eq!(cabin.side.runs.len(), 1, "a typed /btw asks for the side");
    // The side run fails before it says anything.
    let (tx, rx) = mpsc::channel();
    tx.send(grokhub_core::wire::GrokPEvent::Err("boom".into()))
        .expect("send");
    cabin.side.runs[0].cancel.cancel();
    cabin.side.runs[0].rx = Some(rx);
    cabin.poll_side_runs();
    assert!(cabin.side.runs.is_empty(), "no dead side card left behind");
    assert_eq!(cabin.followup_queue, ["what time is it"]);
    reply(cabin, "a1");
    reply(cabin, "It's 9pm.");
    assert_eq!(
        timeline(cabin),
        one_reply_each(&[FIRST, "what time is it"], &["a1", "It's 9pm."])
    );
}

#[test]
fn a_steer_the_engine_never_read_becomes_its_own_next_turn() {
    let mut chat = casual_chat("one-reply-unsteered");
    let cabin = &mut chat.cabin;
    cabin.send_chat("refactor the router into three modules and add tests".into());
    // Steered mid-run, but after the run's last steer check.
    cabin.steer_native_live("also keep the old names".into());
    cabin.requeue_unsteered(vec!["also keep the old names".into()]);
    reply(cabin, "Split into three modules.");
    assert!(cabin.running, "the steer went out as its own turn");
    reply(cabin, "Kept the old names as re-exports.");
    assert_eq!(
        timeline(cabin),
        one_reply_each(
            &[
                "refactor the router into three modules and add tests",
                "also keep the old names"
            ],
            &[
                "Split into three modules.",
                "Kept the old names as re-exports."
            ]
        )
    );
}
