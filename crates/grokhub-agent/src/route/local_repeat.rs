//! Stop a local reply that is stuck repeating, and the sampling limits sent
//! with every on-device chat.
//!
//! The stream guard runs on tokens as they arrive (one llama-server delta,
//! or each copy when a delta is the same symbol over and over). A normal
//! sentence never reaches the limit. The request also carries
//! `repetition_penalty` (and llama-server's `repeat_penalty`) plus
//! `max_tokens`, chosen from the model file name.

use std::borrow::Cow;

use serde_json::{json, Value};

/// Consecutive copies of one token that end the reply. The copy that
/// reaches this count is kept. The next one is dropped.
pub const SAME_TOKEN_LIMIT: usize = 32;

/// Tokens in a short phrase. A phrase whose tokens are all the same is a
/// same-token run, not a phrase.
pub const NGRAM_LEN: usize = 2;

/// How many times that phrase may occur, back to back, before the reply ends.
/// The last copy is kept.
pub const NGRAM_LIMIT: usize = 4;

/// Shown after the words that were kept.
pub const REPEAT_NOTE: &str = "\n\n(Stopped. This reply was repeating.)";

/// What one local chat asks the server to sample with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub repetition_penalty: f64,
    pub max_tokens: u32,
}

/// Penalty and reply cap for `model` (a file name or an API id).
///
/// Hermes loops on one token when the penalty is off, so it gets a slightly
/// stronger penalty and a shorter cap. A 31B-class chat can run longer.
/// Anything else, including the generic `local` id, uses the middle cap.
pub fn sample_for(model: &str) -> Sample {
    let id = model.to_ascii_lowercase();
    if id.contains("hermes") {
        Sample {
            repetition_penalty: 1.15,
            max_tokens: 1024,
        }
    } else if id.contains("31b") || id.contains("70b") || id.contains("72b") || id.contains("405b")
    {
        Sample {
            repetition_penalty: 1.1,
            max_tokens: 4096,
        }
    } else {
        Sample {
            repetition_penalty: 1.1,
            max_tokens: 2048,
        }
    }
}

/// Which name the limits follow: an explicit model id, else the loaded file.
pub fn sample_model<'a>(api_model: &'a str, loaded_file: Option<&str>) -> Cow<'a, str> {
    if api_model != "local" && api_model != "chat" {
        return Cow::Borrowed(api_model);
    }
    match loaded_file {
        Some(name) if !name.is_empty() => Cow::Owned(name.to_string()),
        _ => Cow::Borrowed(api_model),
    }
}

/// Write [`sample_for`] onto a `/v1/chat/completions` body.
///
/// `max_tokens` is left alone when the caller already set one (a short JSON
/// label). Both penalty keys go out: llama-server reads `repeat_penalty`,
/// and the field this app names is `repetition_penalty`.
pub fn apply_sample(body: &mut Value, model: &str) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    let sample = sample_for(model);
    obj.insert(
        "repetition_penalty".into(),
        json!(sample.repetition_penalty),
    );
    obj.insert("repeat_penalty".into(), json!(sample.repetition_penalty));
    obj.entry("max_tokens".to_string())
        .or_insert_with(|| json!(sample.max_tokens));
}

/// Tokens seen so far, only enough to notice a loop.
#[derive(Debug, Default)]
pub struct LoopGuard {
    tokens: Vec<String>,
    stopped: bool,
}

impl LoopGuard {
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Keep `token` until a loop is complete. Returns whether it was kept.
    pub fn push(&mut self, token: &str) -> bool {
        if self.stopped || token.is_empty() {
            return false;
        }
        self.tokens.push(token.to_string());
        if run_len(&self.tokens) >= SAME_TOKEN_LIMIT || phrase_repeated(&self.tokens) {
            self.stopped = true;
        }
        if self.tokens.len() > SAME_TOKEN_LIMIT {
            let extra = self.tokens.len() - SAME_TOKEN_LIMIT;
            self.tokens.drain(0..extra);
        }
        true
    }
}

/// The start of `piece` that is still worth showing. Empty once the reply
/// has already been cut, or when this piece is only the next repeat.
pub fn keep_prefix<'a>(guard: &mut LoopGuard, piece: &'a str) -> &'a str {
    if guard.stopped() || piece.is_empty() {
        return "";
    }
    let spans = token_spans(piece);
    if spans.is_empty() {
        return piece;
    }
    let mut end = 0;
    let mut kept_all = true;
    for (start, stop) in spans {
        if !guard.push(&piece[start..stop]) {
            kept_all = false;
            break;
        }
        end = stop;
        if guard.stopped() {
            kept_all = false;
            break;
        }
    }
    // A piece that was kept in full still has the space after its last word.
    if kept_all {
        return piece;
    }
    &piece[..end]
}

/// Append the note once, when the guard stopped the reply.
pub fn with_note(text: &mut String, stopped: bool) {
    if stopped && !text.contains(REPEAT_NOTE) {
        text.push_str(REPEAT_NOTE);
    }
}

fn run_len(tokens: &[String]) -> usize {
    let Some(last) = tokens.last() else {
        return 0;
    };
    tokens
        .iter()
        .rev()
        .take_while(|token| *token == last)
        .count()
}

fn phrase_repeated(tokens: &[String]) -> bool {
    let n = NGRAM_LEN;
    let need = n.saturating_mul(NGRAM_LIMIT);
    if n == 0 || tokens.len() < need {
        return false;
    }
    let gram = &tokens[tokens.len() - n..];
    if gram.iter().all(|token| token == &gram[0]) {
        return false;
    }
    let mut at = tokens.len() - n;
    for _ in 1..NGRAM_LIMIT {
        if at < n || &tokens[at - n..at] != gram {
            return false;
        }
        at -= n;
    }
    true
}

/// Word spans, or one span per character when a word is the same symbol
/// repeated (an emoji run inside one delta). Whitespace is not a token.
fn token_spans(piece: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = piece.char_indices().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1.is_whitespace() {
            i += 1;
            continue;
        }
        let start_i = i;
        while i < chars.len() && !chars[i].1.is_whitespace() {
            i += 1;
        }
        let word = &chars[start_i..i];
        let repeated_symbol = word.len() > 1
            && !word[0].1.is_alphanumeric()
            && word.iter().all(|(_, ch)| *ch == word[0].1);
        if repeated_symbol {
            for (at, ch) in word {
                spans.push((*at, *at + ch.len_utf8()));
            }
        } else {
            let (at, ch) = word[word.len() - 1];
            spans.push((chars[start_i].0, at + ch.len_utf8()));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(tokens: &[&str]) -> (String, bool) {
        let mut guard = LoopGuard::default();
        let mut text = String::new();
        for token in tokens {
            if guard.push(token) {
                text.push_str(token);
            }
        }
        let stopped = guard.stopped();
        with_note(&mut text, stopped);
        (text, stopped)
    }

    #[test]
    fn a_repeated_token_is_cut_at_32_and_the_rest_is_dropped() {
        let tokens: Vec<&str> = std::iter::repeat_n("😀", 80).collect();
        let (text, stopped) = kept(&tokens);
        assert!(stopped);
        assert_eq!(text.chars().filter(|ch| *ch == '😀').count(), 32);
        assert!(text.ends_with("(Stopped. This reply was repeating.)"));
        assert_eq!(text, format!("{}{REPEAT_NOTE}", "😀".repeat(32)));
    }

    #[test]
    fn one_delta_of_the_same_emoji_stops_at_the_same_count() {
        let mut guard = LoopGuard::default();
        let chunk = "😀".repeat(80);
        let head = keep_prefix(&mut guard, &chunk);
        assert_eq!(head.chars().count(), 32);
        assert!(guard.stopped());
        assert_eq!(keep_prefix(&mut guard, &chunk), "");
        let mut shown = head.to_string();
        with_note(&mut shown, true);
        assert_eq!(shown, format!("{}{REPEAT_NOTE}", "😀".repeat(32)));
    }

    #[test]
    fn a_repeated_phrase_stops_on_its_fourth_copy() {
        let mut words = Vec::new();
        for _ in 0..6 {
            words.push("Go");
            words.push("team");
        }
        let (text, stopped) = kept(&words);
        assert!(stopped);
        assert_eq!(text, format!("GoteamGoteamGoteamGoteam{REPEAT_NOTE}"));
        assert!(!text.contains("GoteamGoteamGoteamGoteamGo"));
    }

    #[test]
    fn a_normal_reply_is_kept_whole() {
        let reply = "I went to the store and then I went home. The cat sat.";
        let words: Vec<&str> = reply.split_whitespace().collect();
        let (text, stopped) = kept(&words);
        assert!(!stopped);
        assert_eq!(text, words.concat());
        assert!(!text.contains("repeating"));
        let mut guard = LoopGuard::default();
        assert_eq!(keep_prefix(&mut guard, reply), reply);
        assert!(!guard.stopped());
    }

    #[test]
    fn sampling_follows_the_model_and_leaves_a_set_cap_alone() {
        assert_eq!(
            sample_for("Hermes-4-14B-Q4_K_M.gguf"),
            Sample {
                repetition_penalty: 1.15,
                max_tokens: 1024,
            }
        );
        assert_eq!(
            sample_for("gemma-4-31b-it-abliterated-q4_k"),
            Sample {
                repetition_penalty: 1.1,
                max_tokens: 4096,
            }
        );
        assert_eq!(
            sample_for("local"),
            Sample {
                repetition_penalty: 1.1,
                max_tokens: 2048,
            }
        );
        assert_eq!(
            sample_model("local", Some("Nous-Hermes-4-14B.gguf")),
            "Nous-Hermes-4-14B.gguf"
        );
        assert_eq!(
            sample_model("Hermes-4-14B.gguf", Some("other.gguf")),
            "Hermes-4-14B.gguf"
        );
        assert_eq!(sample_model("local", None), "local");

        let mut body = json!({"model": "local", "max_tokens": 48});
        apply_sample(&mut body, "Hermes-4-14B.gguf");
        assert_eq!(body["repetition_penalty"].as_f64(), Some(1.15));
        assert_eq!(body["repeat_penalty"].as_f64(), Some(1.15));
        assert_eq!(body["max_tokens"], json!(48));

        let mut open = json!({"model": "local"});
        apply_sample(&mut open, "local");
        assert_eq!(open["max_tokens"], json!(2048));
        assert_eq!(open["repetition_penalty"].as_f64(), Some(1.1));
    }
}
