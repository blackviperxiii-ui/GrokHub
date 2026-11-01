//! The shell's palette answers to the cabin's older words (feed, schedule,
//! keys). `ui/src/paletteSynonyms.json` is the table the palette reads; this
//! checks each word still finds the same row in the cabin's `filter_palette`.

use grokhub_core::shortcuts::filter_palette;
use serde::Deserialize;

#[derive(Deserialize)]
struct Synonym {
    word: String,
    row: String,
    core: String,
}

fn table() -> Vec<Synonym> {
    serde_json::from_str(include_str!("../../ui/src/paletteSynonyms.json"))
        .expect("paletteSynonyms.json parses")
}

#[test]
fn each_synonym_finds_the_same_row_in_the_cabin() {
    let table = table();
    assert_eq!(table.len(), 7);
    for s in &table {
        let labels: Vec<&str> = filter_palette(&s.word)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert!(
            labels.contains(&s.core.as_str()),
            "`{}` finds {labels:?} in the cabin, not {}",
            s.word,
            s.core
        );
    }
}

#[test]
fn the_table_names_these_words() {
    let pairs: Vec<(String, String)> = table().into_iter().map(|s| (s.word, s.row)).collect();
    let want = [
        ("night", "Automations"),
        ("schedule", "Automations"),
        ("ideas", "Pulse"),
        ("feed", "Pulse"),
        ("keys", "Shortcuts"),
        ("keyboard", "Shortcuts"),
        ("hotkeys", "Shortcuts"),
    ];
    assert_eq!(
        pairs,
        want.map(|(w, r)| (w.to_string(), r.to_string())).to_vec()
    );
    // A literal check that the cabin's row is the one named, not only some row.
    assert_eq!(filter_palette("feed"), vec![("Pulse", "nav:pulse")]);
    assert_eq!(
        filter_palette("schedule"),
        vec![("Automations", "nav:night")]
    );
    assert_eq!(
        filter_palette("hotkeys"),
        vec![("Keyboard shortcuts", "shortcuts")]
    );
}
