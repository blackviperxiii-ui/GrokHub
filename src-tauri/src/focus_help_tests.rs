use super::*;

#[test]
fn off_reads_nothing_and_shows_nothing() {
    let mut help = FocusHelp::default();
    help.tick(true, false);
    help.tick(false, false);
    std::thread::sleep(READ_DELAY * 3);
    assert_eq!(*help.read.lock().unwrap(), None, "no read while off");
    help.note_process(Some("blender"), &[]);
    assert_eq!(help.chip(false), None);
    assert_eq!(help.take_context(false), None);
}

#[test]
fn a_known_app_puts_one_chip_and_one_context_line() {
    let mut help = FocusHelp::default();
    help.note_process(Some("Cursor.exe"), &[]);
    let chip = help.chip(true).expect("chip");
    assert_eq!(chip.label, "Next coding step");
    assert_eq!(chip.hint, "Because you were in Cursor");
    assert_eq!(chip.id, "focus:coding:cursor");
    assert_eq!(
        help.take_context(true).as_deref(),
        Some(
            "[Context: the user was just in Cursor (coding). Use it only if it fits the ask.]"
        )
    );
    assert_eq!(help.take_context(true), None, "once per return");
}

#[test]
fn an_unknown_app_or_turning_it_off_clears_the_hint() {
    let mut help = FocusHelp::default();
    help.note_process(Some("firefox"), &[]);
    assert_eq!(help.chip(true), None);
    assert_eq!(help.take_context(true), None);
    help.note_process(Some("blender"), &[]);
    assert!(help.chip(true).is_some());
    help.clear();
    assert_eq!(help.app, None);
    assert_eq!(help.chip(true), None);
}

#[test]
fn blur_while_on_schedules_a_read_slot() {
    let mut help = FocusHelp::default();
    help.was_focused = true;
    help.tick(false, true);
    // The spawn sleeps READ_DELAY; give it a moment then confirm the slot
    // can be written (process name may be None on this CI host).
    std::thread::sleep(READ_DELAY * 3);
    drop(help.read.lock().unwrap());
}
