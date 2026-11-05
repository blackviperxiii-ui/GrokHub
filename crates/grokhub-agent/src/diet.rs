//! Prompt diet (round 3 item 18): a leaner fixed prompt and sparse `read_file`
//! line numbers. Both are off by default. `prompt_diet.toml` in the config
//! folder turns each on (`lean_prompt = true`, `sparse_line_numbers = true`),
//! and only the paired eval decides whether either ships on.

use std::path::Path;

/// The switch file in the config folder.
pub const CONFIG_NAME: &str = "prompt_diet.toml";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings {
    /// Send the lean fixed prompt: same intro and safety block, less coaching.
    pub lean_prompt: bool,
    /// Number every tenth line of a `read_file` result instead of every line.
    pub sparse_line_numbers: bool,
}

pub fn settings(config_dir: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(config_dir.join(CONFIG_NAME)) else {
        return Settings::default();
    };
    let file =
        crate::config_file::parse(CONFIG_NAME, &text, &["lean_prompt", "sparse_line_numbers"]);
    Settings {
        lean_prompt: file.flag("lean_prompt", false),
        sparse_line_numbers: file.flag("sparse_line_numbers", false),
    }
}

/// The settings in this thread's config folder.
pub fn current() -> Settings {
    settings(&crate::perm::config_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_switches_are_off_unless_the_file_turns_them_on() {
        let dir = std::env::temp_dir().join(format!("grokhub-diet-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(settings(&dir), Settings::default());
        std::fs::write(dir.join(CONFIG_NAME), "lean_prompt = true\n").expect("write");
        assert_eq!(
            settings(&dir),
            Settings {
                lean_prompt: true,
                sparse_line_numbers: false
            }
        );
        std::fs::write(
            dir.join(CONFIG_NAME),
            "lean_prompt = flase\nsparse_line_numbers = on\n",
        )
        .expect("write");
        assert_eq!(
            settings(&dir),
            Settings {
                lean_prompt: false,
                sparse_line_numbers: true
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
