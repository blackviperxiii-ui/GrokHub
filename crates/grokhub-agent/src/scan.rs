#[cfg(test)]
mod tests {
    use std::path::Path;

    fn banned() -> [&'static str; 4] {
        [
            concat!("auth", ".json"),
            concat!("config", ".toml"),
            concat!("cli-chat-", "proxy"),
            concat!("GROK", "_HOME"),
        ]
    }

    fn walk(dir: &Path) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if path.file_name().and_then(|s| s.to_str()) == Some("target") {
                continue;
            }
            if path.is_dir() {
                walk(&path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !(name.ends_with(".rs") || name.ends_with(".md") || name == "NOTICE") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for banned in banned() {
                assert!(!text.contains(banned), "{} contains {banned}", path.display());
            }
        }
    }

    #[test]
    fn native_sources_never_name_cli_auth_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        walk(root);
        let extra = [
            include_str!("../../grokhub-app/src/app/native_engine.rs"),
            include_str!("../../grokhub-app/src/engine_handle.rs"),
            include_str!("../../grokhub-core/src/xai_signin.rs"),
        ];
        for text in extra {
            for banned in banned() {
                assert!(!text.contains(banned), "{banned}");
            }
        }
    }
}
