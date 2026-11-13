use super::*;

/// A safetensors file with one F32 `embeddings` tensor.
pub(crate) fn safetensors(rows: &[Vec<f32>]) -> Vec<u8> {
    let dim = rows.first().map_or(0, Vec::len);
    let data: Vec<u8> = rows
        .iter()
        .flatten()
        .flat_map(|x| x.to_le_bytes())
        .collect();
    let header = format!(
        "{{\"embeddings\":{{\"dtype\":\"F32\",\"shape\":[{},{dim}],\"data_offsets\":[0,{}]}}}}",
        rows.len(),
        data.len()
    );
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&data);
    out
}

const VOCAB: &str = "[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\n,\n.\n?\nthe\ngpu\ngraphics\ncard\nplay\n##ing\nun\n##aff\n##able\ncafe\n";

fn model() -> StaticModel {
    let n = VOCAB.lines().count();
    let rows: Vec<Vec<f32>> = (0..n)
        .map(|i| {
            let mut v = vec![0.0; 4];
            v[i % 4] = 1.0 + i as f32;
            v
        })
        .collect();
    StaticModel::from_parts(VOCAB, &safetensors(&rows)).unwrap()
}

fn id(tok: &str) -> u32 {
    VOCAB.lines().position(|l| l == tok).unwrap() as u32
}

#[test]
fn basic_tokens_lowercase_strip_accents_and_split_punctuation() {
    assert_eq!(
        basic_tokens("  Café,PLAYING?\tthe GPU\u{0}"),
        vec!["cafe", ",", "playing", "?", "the", "gpu"]
    );
    // Category S symbols stay inside the word; P splits.
    assert_eq!(basic_tokens("a©b «c»"), vec!["a©b", "«", "c", "»"]);
    assert_eq!(basic_tokens("x\u{0301}y"), vec!["xy"]);
    assert_eq!(basic_tokens("中文"), vec!["中", "文"]);
}

#[test]
fn wordpiece_takes_the_longest_piece_and_drops_unknown_words() {
    let m = model();
    assert_eq!(m.tokenize("Playing"), vec![id("play"), id("##ing")]);
    assert_eq!(
        m.tokenize("unaffable"),
        vec![id("un"), id("##aff"), id("##able")]
    );
    // `zzz` has no piece, so the whole word is [UNK] and is dropped.
    assert_eq!(m.tokenize("the zzz gpu"), vec![id("the"), id("gpu")]);
    // A word with an unknown tail is [UNK] as a whole, not its known prefix.
    assert_eq!(m.tokenize("playx"), Vec::<u32>::new());
}

#[test]
fn tokens_cap_at_max_tokens() {
    let m = model();
    let text = "the ".repeat(MAX_TOKENS + 40);
    assert_eq!(m.tokenize(&text).len(), MAX_TOKENS);
}

#[test]
fn encode_is_the_unit_mean_and_none_without_known_tokens() {
    let m = model();
    let v = m.encode("gpu").unwrap();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
    assert!((cosine(&v, &m.encode("GPU!").unwrap()) - 1.0).abs() < 1e-5);
    assert!(m.encode("zzz qqq").is_none());
    assert!(m.encode("").is_none());
}

#[test]
fn blobs_round_trip_and_reject_a_wrong_width() {
    let v = vec![0.5f32, -1.25, 3.0];
    assert_eq!(from_blob(&to_blob(&v), 3), Some(v));
    assert_eq!(from_blob(&[0u8; 7], 2), None);
}

#[test]
fn from_parts_refuses_mismatched_or_non_f32_tables() {
    let rows = vec![vec![1.0f32, 0.0]; 3];
    assert!(StaticModel::from_parts("a\nb\n", &safetensors(&rows))
        .unwrap_err()
        .contains("3 vectors for 2"));
    let mut bad = safetensors(&rows);
    let at = bad.windows(3).position(|w| w == b"F32").unwrap();
    bad[at..at + 3].copy_from_slice(b"F16");
    assert!(StaticModel::from_parts("a\nb\nc\n", &bad)
        .unwrap_err()
        .contains("only F32"));
    assert!(StaticModel::from_parts("a\n", &[1, 2, 3]).is_err());
    let mut long = 1_000_000u64.to_le_bytes().to_vec();
    long.extend_from_slice(b"{}");
    assert!(StaticModel::from_parts("a\n", &long).is_err());
}

#[test]
fn pins_check_size_sha256_and_git_blob_ids() {
    // `git hash-object` of "hello\n" is ce0136…; SHA-256 of "hello\n" is 5891b5…
    let blob = ModelFile {
        name: "v",
        bytes: 6,
        pin: Pin::GitBlob("ce013625030ba8dba906f756967f9e9ca394464a"),
    };
    let sha = ModelFile {
        name: "w",
        bytes: 6,
        pin: Pin::Sha256("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"),
    };
    assert!(verify(&blob, b"hello\n"));
    assert!(verify(&sha, b"hello\n"));
    assert!(!verify(&blob, b"hellO\n"));
    assert!(!verify(&sha, b"hello"));
    let dir = std::env::temp_dir().join(format!("gh-embed-pin-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("v"), b"hello\n").unwrap();
    assert!(verify_path(&blob, &dir.join("v")));
    assert!(verify_path(&sha, &dir.join("v")));
    fs::write(dir.join("v"), b"jello\n").unwrap();
    assert!(!verify_path(&blob, &dir.join("v")));
    assert!(!verify_path(&blob, &dir.join("missing")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_pinned_model_is_potion_base_8m_with_both_files_pinned() {
    let m = POTION_BASE_8M;
    assert_eq!(m.dim, 256);
    let names: Vec<&str> = m.files.iter().map(|f| f.name).collect();
    assert_eq!(names, vec![VOCAB_FILE, WEIGHTS_FILE]);
    // 29,528 rows of 256 F32 values, plus the header.
    let weights = m.files[1];
    assert!(weights.bytes > 29_528 * 256 * 4 && weights.bytes < 29_528 * 256 * 4 + 256);
    assert_eq!(
        m.url(&m.files[0]),
        "https://huggingface.co/minishlab/potion-base-8M/resolve/main/vocab.txt"
    );
    let dir = m.dir(Path::new("/cfg"));
    assert!(dir.ends_with("models/embed/potion-base-8M"));
    assert!(!m.present(&dir));
}

#[test]
fn load_refuses_files_that_fail_their_pin() {
    let dir = std::env::temp_dir().join(format!("gh-embed-load-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(VOCAB_FILE), "a\n").unwrap();
    fs::write(dir.join(WEIGHTS_FILE), b"x").unwrap();
    let err = StaticModel::load(&POTION_BASE_8M, &dir).unwrap_err();
    assert!(err.contains("failed its check"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}
