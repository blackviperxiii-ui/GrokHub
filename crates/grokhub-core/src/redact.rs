const PATTERNS: &[(&str, &str)] = &[
    ("sk-", "[redacted]"),
    ("xai-", "[redacted]"),
    ("ghp_", "[redacted]"),
    ("Bearer ", "Bearer [redacted]"),
];

pub fn redact_secrets(input: &str) -> String {
    let mut s = input.to_string();
    for (needle, _) in PATTERNS {
        loop {
            let Some(idx) = s.find(needle) else {
                break;
            };
            let rest = &s[idx + needle.len()..];
            let n = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(*c, '-' | '_' | '.' | '~' | '+' | '/' | '='))
                .count();
            if n < 12 {
                break;
            }
            let end = idx + needle.len() + rest.chars().take(n).map(|c| c.len_utf8()).sum::<usize>();
            s.replace_range(idx..end, "[redacted]");
        }
    }
    s
}

pub fn is_plain_text(s: &str) -> bool {
    redact_secrets(s) == s
}

pub fn forget_topic(markdown: &str, topic: &str) -> String {
    let t = topic.trim().to_ascii_lowercase();
    if t.is_empty() {
        return markdown.to_string();
    }
    let mut out = String::new();
    for line in markdown.lines() {
        if line.to_ascii_lowercase().contains(&t) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sk() {
        let out = redact_secrets("token sk-abcdefghijklmnopqrstuv");
        assert!(!out.contains("sk-abcdefghijklmnopqrstuv"));
        let kept = forget_topic("editor: nvim\nwifi printer in den\n", "wifi");
        assert!(kept.contains("nvim"));
        assert!(!kept.contains("wifi"));
    }

    #[test]
    fn redacts_xai_github_and_bearer() {
        let xai = redact_secrets("key xai-abcdefghijklmnopqrstuv");
        assert!(!xai.contains("xai-abcdefghijklmnopqrstuv"), "{xai}");
        let ghp = redact_secrets("export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuv");
        assert!(!ghp.contains("ghp_abcdefghijklmnopqrstuv"), "{ghp}");
        let bearer = redact_secrets("Authorization: Bearer abcdefghijklmnop");
        assert!(!bearer.contains("abcdefghijklmnop"), "{bearer}");
        assert!(
            is_plain_text("sk-short"),
            "short tokens stay visible so ordinary words are not eaten"
        );
        assert!(is_plain_text("xai-tiny"));
        let two = redact_secrets("sk-abcdefghijklmnopqrstuv and sk-zyxwvutsrqponmlkjih");
        assert!(!two.contains("sk-"), "{two}");
        assert_eq!(two.matches("[redacted]").count(), 2);
    }
}
