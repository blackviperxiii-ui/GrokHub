//! Settings → General and the composer's model chip: the chat models to pick
//! from. The catalog always shows; with a saved key the list adds the models
//! that key can use, from the router's guarded `XaiApiSource` (no new
//! outbound call). The picker never switches models by itself.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_core::model_registry::discover::CatalogSource;
use grokhub_core::model_registry::ModelMeta;
use grokhub_core::models::MODEL_CATALOG;
use serde::Serialize;

/// The account's list is read again after this long, or on Refresh.
pub const CACHE_FOR: Duration = Duration::from_secs(10 * 60);

pub const NO_KEY_NOTE: &str =
    "Add an xAI API key in Settings → Account to see every model your account can use.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The built-in Grok catalog.
    Catalog,
    /// Listed for the saved key.
    Account,
    /// A model id typed under Other…, kept as it is.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    /// The model chat uses now.
    pub current: String,
    pub options: Vec<ModelOption>,
    /// Why the account's models are missing, when they are.
    pub note: Option<String>,
}

/// The catalog's chat rows, in catalog order.
pub fn catalog() -> Vec<ModelOption> {
    MODEL_CATALOG
        .iter()
        .filter(|m| m.kind == "chat")
        .map(|m| ModelOption {
            id: m.id.to_string(),
            label: m.label.to_string(),
            source: Source::Catalog,
        })
        .collect()
}

/// Image, video, voice and embedding models can't hold a chat.
fn is_chat_model(meta: &ModelMeta) -> bool {
    let id = meta.id.to_ascii_lowercase();
    let not_chat = ["imagine", "image", "video", "voice", "embed", "tts", "stt"];
    if not_chat.iter().any(|word| id.contains(word)) {
        return false;
    }
    match &meta.output_modalities {
        Some(out) => out.iter().any(|m| m.eq_ignore_ascii_case("text")),
        None => true,
    }
}

/// The catalog first, then the account's other chat models by id, each once.
/// A catalog model the account also lists keeps its catalog label.
pub fn merge(catalog: Vec<ModelOption>, live: &[ModelMeta]) -> Vec<ModelOption> {
    let mut out = catalog;
    let mut extra: Vec<ModelOption> = live
        .iter()
        .filter(|m| is_chat_model(m))
        .filter(|m| !m.id.trim().is_empty())
        .map(|m| ModelOption {
            id: m.id.trim().to_string(),
            label: m.id.trim().to_string(),
            source: Source::Account,
        })
        .collect();
    extra.sort_by(|a, b| a.id.cmp(&b.id));
    for option in extra {
        if !out.iter().any(|o| o.id == option.id) {
            out.push(option);
        }
    }
    out
}

/// The options with `current` in them: an id that isn't listed shows as Custom.
pub fn with_current(mut options: Vec<ModelOption>, current: &str) -> Vec<ModelOption> {
    let current = current.trim();
    if !current.is_empty() && !options.iter().any(|o| o.id == current) {
        options.push(ModelOption {
            id: current.to_string(),
            label: current.to_string(),
            source: Source::Custom,
        });
    }
    options
}

/// The page's list. `source` is the account's listing (None with no key);
/// a failing source still returns the catalog, with the reason as the note.
pub fn list(current: &str, source: Option<&dyn CatalogSource>) -> ModelList {
    let (live, note) = match source {
        None => (Vec::new(), Some(NO_KEY_NOTE.to_string())),
        Some(source) => match source.fetch() {
            Ok(listing) => (listing.models, None),
            Err(err) => (
                Vec::new(),
                Some(format!(
                    "Couldn't read your account's models ({err}). Showing the built-in list."
                )),
            ),
        },
    };
    ModelList {
        current: current.trim().to_string(),
        options: with_current(merge(catalog(), &live), current),
        note,
    }
}

/// A model id from the picker or Other…, trimmed. Unknown ids are kept.
pub fn clean_id(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("Pick a model first.".into());
    }
    if id.len() > 120 || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("A model id is one word, like grok-4.3.".into());
    }
    Ok(id.to_string())
}

/// The account's listing, kept for `CACHE_FOR` so opening the picker twice
/// asks once. Keyed by a hash of the bearer, so a new key reads again.
static CACHE: Mutex<Option<(Instant, String, Vec<ModelMeta>)>> = Mutex::new(None);

/// A source that answers from the cache when it is fresh for this bearer,
/// and fills it from `inner` otherwise.
pub struct Cached<'a> {
    pub inner: &'a dyn CatalogSource,
    pub bearer_tag: String,
    pub refresh: bool,
}

impl CatalogSource for Cached<'_> {
    fn kind(&self) -> grokhub_core::model_registry::SourceKind {
        self.inner.kind()
    }

    fn fetch(&self) -> Result<grokhub_core::model_registry::discover::Listing, String> {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if !self.refresh {
            if let Some((at, tag, models)) = cache.as_ref() {
                if *tag == self.bearer_tag && at.elapsed() < CACHE_FOR {
                    return Ok(grokhub_core::model_registry::discover::Listing::new(
                        self.kind(),
                        models.clone(),
                    ));
                }
            }
        }
        let listing = self.inner.fetch()?;
        *cache = Some((
            Instant::now(),
            self.bearer_tag.clone(),
            listing.models.clone(),
        ));
        Ok(listing)
    }
}

/// A short tag for the bearer, so the cache never holds the key itself.
pub fn bearer_tag(bearer: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bearer.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::discover::Listing;
    use grokhub_core::model_registry::SourceKind;
    use std::cell::Cell;

    struct Fake {
        rows: Result<Vec<ModelMeta>, String>,
        calls: Cell<u32>,
    }

    impl CatalogSource for Fake {
        fn kind(&self) -> SourceKind {
            SourceKind::XaiApi
        }
        fn fetch(&self) -> Result<Listing, String> {
            self.calls.set(self.calls.get() + 1);
            self.rows
                .clone()
                .map(|rows| Listing::new(SourceKind::XaiApi, rows))
        }
    }

    fn fake(rows: Result<Vec<ModelMeta>, String>) -> Fake {
        Fake {
            rows,
            calls: Cell::new(0),
        }
    }

    fn ids(list: &ModelList) -> Vec<&str> {
        list.options.iter().map(|o| o.id.as_str()).collect()
    }

    #[test]
    fn signed_out_shows_the_catalog_and_says_why() {
        let list = list("grok-4.3", None);
        assert_eq!(
            ids(&list),
            vec!["grok-3-mini-fast", "grok-4.3", "grok-4.6", "grok-4.7"]
        );
        assert!(list.options.iter().all(|o| o.source == Source::Catalog));
        assert_eq!(list.note.as_deref(), Some(NO_KEY_NOTE));
        assert_eq!(list.current, "grok-4.3");
    }

    #[test]
    fn account_models_merge_once_after_the_catalog() {
        let mut image = ModelMeta::bare("grok-imagine-image-2.0");
        image.output_modalities = Some(vec!["image".into()]);
        let mut audio_only = ModelMeta::bare("grok-audio-1");
        audio_only.output_modalities = Some(vec!["audio".into()]);
        let source = fake(Ok(vec![
            ModelMeta::bare("grok-4.3"),
            ModelMeta::bare("grok-5-preview"),
            ModelMeta::bare("grok-4-fast"),
            image,
            audio_only,
            ModelMeta::bare("grok-voice-latest"),
        ]));
        let list = list("grok-4.3", Some(&source));
        assert_eq!(list.note, None);
        let tail: Vec<_> = list.options[4..].iter().collect();
        assert_eq!(
            tail,
            vec![
                &ModelOption {
                    id: "grok-4-fast".into(),
                    label: "grok-4-fast".into(),
                    source: Source::Account,
                },
                &ModelOption {
                    id: "grok-5-preview".into(),
                    label: "grok-5-preview".into(),
                    source: Source::Account,
                },
            ]
        );
        // The catalog row the account also lists keeps its label, once.
        let grok43: Vec<_> = list.options.iter().filter(|o| o.id == "grok-4.3").collect();
        assert_eq!(grok43.len(), 1);
        assert_eq!(grok43[0].label, "Grok 4.3");
        assert_eq!(grok43[0].source, Source::Catalog);
    }

    #[test]
    fn a_failing_source_still_returns_the_catalog() {
        let source = fake(Err("HTTP 401".into()));
        let list = list("grok-4.3", Some(&source));
        assert_eq!(list.options.len(), 4);
        assert_eq!(
            list.note.as_deref(),
            Some("Couldn't read your account's models (HTTP 401). Showing the built-in list.")
        );
    }

    #[test]
    fn a_custom_current_model_is_listed_as_custom() {
        let list = list("my-tuned-grok", None);
        assert_eq!(
            list.options.last(),
            Some(&ModelOption {
                id: "my-tuned-grok".into(),
                label: "my-tuned-grok".into(),
                source: Source::Custom,
            })
        );
    }

    #[test]
    fn no_local_model_is_offered() {
        let list = list("grok-4.3", None);
        assert!(list.options.iter().all(|o| !o.id.starts_with("local")));
    }

    #[test]
    fn model_ids_are_trimmed_and_checked() {
        assert_eq!(clean_id("  grok-x  "), Ok("grok-x".to_string()));
        assert_eq!(clean_id("   "), Err("Pick a model first.".to_string()));
        assert_eq!(
            clean_id("grok 4"),
            Err("A model id is one word, like grok-4.3.".to_string())
        );
    }

    #[test]
    fn the_cache_answers_a_second_open_and_refresh_reads_again() {
        let inner = fake(Ok(vec![ModelMeta::bare("grok-cache-test")]));
        let tag = bearer_tag("sk-abcdefghijklmnopqrstuv-cache");
        let first = Cached {
            inner: &inner,
            bearer_tag: tag.clone(),
            refresh: true,
        };
        assert_eq!(first.fetch().map(|l| l.models.len()), Ok(1));
        let again = Cached {
            inner: &inner,
            bearer_tag: tag.clone(),
            refresh: false,
        };
        assert_eq!(
            again.fetch().map(|l| l.models[0].id.clone()),
            Ok("grok-cache-test".into())
        );
        assert_eq!(inner.calls.get(), 1);
        let other_key = Cached {
            inner: &inner,
            bearer_tag: bearer_tag("sk-zyxwvutsrqponmlkjihg-other"),
            refresh: false,
        };
        assert!(other_key.fetch().is_ok());
        assert_eq!(inner.calls.get(), 2);
        assert_ne!(tag, "sk-abcdefghijklmnopqrstuv-cache");
    }
}
