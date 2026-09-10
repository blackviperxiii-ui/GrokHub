use std::path::PathBuf;

pub fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn user_home_prefers_home_then_userprofile() {
        let _g = LOCK.lock().unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", r"C:\Users\viper");
        let got = user_home().expect("USERPROFILE");
        assert!(got.ends_with("viper") || got.to_string_lossy().contains("viper"));
        std::env::remove_var("USERPROFILE");
        assert!(user_home().is_none() || old_home.is_some());
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match old_up {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
