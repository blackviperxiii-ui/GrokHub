use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Plain-word chat model benching state and persistence.
/// Full UI/orchestrator wiring is documented in this patch so callers
/// can check `is_benched` before routing and handle `parse_bench_command` in chat.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BenchedModels {
    pub models: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchAction {
    Bench(String),
    Unbench(String),
}

impl BenchedModels {
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };
        let models = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| l.to_ascii_lowercase())
            .collect();
        Self { models }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = self.models.iter().cloned().collect::<Vec<_>>().join("\n");
        fs::write(path, body)
    }
}

pub fn parse_bench_command(input: &str) -> Option<BenchAction> {
    let lower = input.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("unbench ") {
        let model = rest.strip_prefix("model ").unwrap_or(rest).trim();
        if !model.is_empty() {
            return Some(BenchAction::Unbench(model.to_string()));
        }
    } else if let Some(rest) = lower.strip_prefix("bench ") {
        let model = rest.strip_prefix("model ").unwrap_or(rest).trim();
        if !model.is_empty() {
            return Some(BenchAction::Bench(model.to_string()));
        }
    }
    None
}

pub fn apply(benched: &mut BenchedModels, action: &BenchAction) -> bool {
    match action {
        BenchAction::Bench(model) => benched.models.insert(model.trim().to_ascii_lowercase()),
        BenchAction::Unbench(model) => benched.models.remove(&model.trim().to_ascii_lowercase()),
    }
}

pub fn is_benched(benched: &BenchedModels, model: &str) -> bool {
    benched.models.contains(&model.trim().to_ascii_lowercase())
}

pub fn confirmation_message(action: &BenchAction) -> String {
    match action {
        BenchAction::Bench(model) => format!("Benched model '{model}' from chat routing."),
        BenchAction::Unbench(model) => format!("Unbenched model '{model}' for chat routing."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_persists_bench_and_unbench() {
        let dir = std::env::temp_dir().join("grokhub_user_bench_test");
        let path = dir.join("benched.txt");
        let _ = fs::remove_file(&path);

        let bench_cmd = parse_bench_command("bench grok-4").expect("bench cmd");
        let mut state = BenchedModels::default();
        assert!(apply(&mut state, &bench_cmd));
        assert!(is_benched(&state, "Grok-4"));
        assert_eq!(confirmation_message(&bench_cmd), "Benched model 'grok-4' from chat routing.");
        state.save(&path).expect("save bench");

        let mut loaded = BenchedModels::load(&path);
        assert!(is_benched(&loaded, "grok-4"));

        let unbench_cmd = parse_bench_command("unbench model grok-4").expect("unbench cmd");
        assert!(apply(&mut loaded, &unbench_cmd));
        assert!(!is_benched(&loaded, "grok-4"));
        loaded.save(&path).expect("save unbench");

        let reloaded = BenchedModels::load(&path);
        assert!(!is_benched(&reloaded, "grok-4"));
        let _ = fs::remove_dir_all(&dir);
    }
}
