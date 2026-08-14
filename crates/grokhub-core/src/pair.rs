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
