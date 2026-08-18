//! One-shot CONSULT — no spawn tree, no host.

pub fn parse_consult(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("CONSULT:") {
            let q = rest.trim().trim_matches('{').trim_matches('}').trim();
            if !q.is_empty() {
                return Some(q.to_string());
            }
        }
    }
    None
}

pub fn format_consult_reply(question: &str, reply: &str) -> String {
    format!("CONSULT_RESULT:\nQ: {question}\nA: {}", reply.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consult_line() {
        assert_eq!(
            parse_consult("ok\nCONSULT: should I flash now\n").as_deref(),
            Some("should I flash now")
        );
        assert!(parse_consult("hello").is_none());
        assert!(format_consult_reply("q", "a").contains("CONSULT_RESULT"));
    }
}
