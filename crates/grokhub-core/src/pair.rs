use getrandom::getrandom;

pub const CODE_ALPH: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const PAIR_TTL_MS: u64 = 15 * 60 * 1000;

pub fn normalize_code(c: &str) -> String {
    c.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}

pub fn make_pair_code() -> String {
    let mut bytes = [0u8; 6];
    let _ = getrandom(&mut bytes);
    let alph = CODE_ALPH.as_bytes();
    let mut s = String::with_capacity(7);
    for (i, b) in bytes.iter().enumerate() {
        if i == 3 {
            s.push('-');
        }
        s.push(alph[(*b as usize) % alph.len()] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_and_upcases() {
        assert_eq!(normalize_code("abc-234"), "ABC234");
        assert_eq!(normalize_code("ab c-23 4"), "ABC234");
        assert_eq!(normalize_code(""), "");
    }

    #[test]
    fn make_pair_code_has_dash_and_alphabet() {
        let code = make_pair_code();
        let b = code.as_bytes();
        assert_eq!(b.len(), 7);
        assert_eq!(b[3], b'-');
        assert!(code.chars().filter(|c| *c != '-').all(|c| CODE_ALPH.contains(c)));
        assert_eq!(PAIR_TTL_MS, 15 * 60 * 1000);
    }
}
