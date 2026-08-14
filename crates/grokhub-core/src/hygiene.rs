//! Skip lock / password frames before they reach the model.

pub fn should_send_screenshot(window_title: &str, ocr_text: &str) -> bool {
    !lockish(window_title) && !lockish(ocr_text)
}

pub fn lockish(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("lock")
        || s.contains("password")
        || s.contains("sudo")
        || s.contains("authentication")
        || s.contains("polkit")
        || s.contains("passcode")
        || s.contains("unlock")
        || s.contains("greeter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_lock_and_password() {
        assert!(!should_send_screenshot("Lock screen", ""));
        assert!(!should_send_screenshot("Terminal", "enter password"));
        assert!(!should_send_screenshot("polkit agent", ""));
        assert!(should_send_screenshot("nvim — cabin.rs", "fn main"));
    }
}
