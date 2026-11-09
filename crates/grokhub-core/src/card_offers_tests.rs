use super::*;

const NOW: u64 = 1_800_000_000_000;

fn card(id: &str, kind: &str, title: &str, age_ms: u64) -> UpdateCard {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "kind": kind,
        "title": title,
        "createdAt": NOW - age_ms,
        "status": "unread",
    }))
    .expect("card")
}

#[test]
fn a_reminder_gets_snooze_only() {
    let c = card(
        "a",
        "suggestion",
        "Remind me to call the shop tomorrow",
        60_000,
    );
    let o = offers(&c, std::slice::from_ref(&c), NOW);
    assert_eq!(
        o,
        Offers {
            snooze: true,
            delete: false
        }
    );
}

#[test]
fn an_expiring_card_is_time_bound() {
    let mut c = card("a", "suggestion", "Plain title", 60_000);
    c.expires_at = Some(NOW + 3_600_000);
    assert!(offers(&c, &[], NOW).snooze);
    assert!(!offers(&c, &[], NOW).delete);
}

#[test]
fn a_stale_card_gets_delete_not_snooze() {
    let c = card("a", "suggestion", "Sale ends tonight", STALE_MS + 1);
    assert_eq!(
        offers(&c, &[], NOW),
        Offers {
            snooze: false,
            delete: true
        }
    );
}

#[test]
fn a_duplicate_gets_delete() {
    let a = card("a", "suggestion", "Same  Title", 60_000);
    let b = card("b", "suggestion", "same title", 120_000);
    let all = [a.clone(), b];
    assert_eq!(
        offers(&a, &all, NOW),
        Offers {
            snooze: false,
            delete: true
        }
    );
}

#[test]
fn a_finished_one_off_gets_delete() {
    let c = card("a", "automation_done", "Board summary", 60_000);
    assert_eq!(
        offers(&c, &[], NOW),
        Offers {
            snooze: false,
            delete: true
        }
    );
}

#[test]
fn unsure_means_both() {
    let c = card("a", "digest", "Three new posts about gardening", 60_000);
    assert_eq!(
        offers(&c, &[], NOW),
        Offers {
            snooze: true,
            delete: true
        }
    );
}

#[test]
fn time_words_match_whole_words_only() {
    // "due" inside "residue" and "later" inside "translate" are not times.
    let c = card("a", "suggestion", "Translate the residue notes", 60_000);
    assert!(!time_bound(&c));
}
