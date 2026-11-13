//! Known xAI list prices and context windows (from the built-in profiles,
//! `data/model_profiles.toml`), used only where no xAI API listing describes the model (a Grok
//! Build sign-in lists bare ids) and for the eval's cost estimate. A row the
//! xAI API lists keeps its own figures: the live listing always wins.

use super::record::{ModelMeta, Prices};

/// xAI bills every token of a request (cached ones too) at twice the rate
/// once its prompt reaches this many tokens (docs.x.ai pricing, Grok 4 models).
pub const LONG_CONTEXT_TOKENS: u64 = 200_000;

/// A context window this large holds a long prompt with room to grow, so the
/// router's 200k gate prefers it.
pub const MILLION_CONTEXT: u64 = 1_000_000;

/// The xAI row for a bare id (a dated or `-latest` form, or an alias),
/// from the built-in profiles. An id with a provider prefix is never a
/// direct xAI model.
fn row(id: &str) -> Option<super::builtin::Profile> {
    if id.contains('/') {
        return None;
    }
    super::builtin::get(id).filter(|p| p.provider.as_deref() == Some("xai"))
}

/// The seed prices of `id` (or a dated / `-latest` form of it), with the
/// long-context step the profile states (2x at 200k for the Grok 4 line).
pub fn seed_prices(id: &str) -> Option<Prices> {
    let p = row(id)?.prices();
    (!p.is_empty()).then_some(p)
}

/// The seed context window of `id`, in tokens.
pub fn seed_context(id: &str) -> Option<u64> {
    row(id)?.context
}

/// Fill a row no xAI API listing described: the whole price block when it
/// has no prices, and the context window when it has none. Known fields stay.
pub fn fill(meta: &mut ModelMeta) {
    if meta.prices.is_empty() {
        if let Some(p) = seed_prices(&meta.id) {
            meta.prices = p;
        }
    }
    if meta.context_length.is_none() {
        meta.context_length = seed_context(&meta.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_prices_and_windows_are_the_published_ones() {
        assert_eq!(
            seed_prices("grok-4.7"),
            Some(Prices {
                prompt: Some(20_000),
                cached: Some(5_000),
                cache_write: None,
                completion: Some(60_000),
                prompt_long: Some(40_000),
                completion_long: Some(120_000),
                long_context_threshold: Some(200_000),
            })
        );
        assert_eq!(
            seed_prices("grok-4.5-latest"),
            Some(Prices {
                prompt: Some(20_000),
                cached: Some(3_000),
                cache_write: None,
                completion: Some(60_000),
                prompt_long: Some(40_000),
                completion_long: Some(120_000),
                long_context_threshold: Some(200_000),
            })
        );
        assert_eq!(
            seed_prices("grok-4.3-0414"),
            Some(Prices {
                prompt: Some(12_500),
                cached: Some(2_000),
                cache_write: None,
                completion: Some(25_000),
                prompt_long: Some(25_000),
                completion_long: Some(50_000),
                long_context_threshold: Some(200_000),
            })
        );
        assert_eq!(
            [
                seed_context("grok-4.7"),
                seed_context("grok-4.5"),
                seed_context("grok-4.3")
            ],
            [Some(500_000), Some(500_000), Some(1_000_000)]
        );
        // A Fast variant or an unknown id has no seed: it stays unknown.
        assert_eq!(seed_prices("grok-4.7-fast"), None);
        assert_eq!(seed_context("vendor/grok-4.7"), None);
    }

    #[test]
    fn fill_keeps_what_a_listing_gave() {
        let mut bare = ModelMeta::bare("grok-4.3");
        fill(&mut bare);
        assert_eq!(
            (bare.prices.prompt, bare.context_length),
            (Some(12_500), Some(1_000_000))
        );
        let mut listed = ModelMeta {
            id: "grok-4.7".into(),
            context_length: Some(256_000),
            prices: Prices {
                prompt: Some(30_000),
                ..Prices::default()
            },
            ..ModelMeta::default()
        };
        fill(&mut listed);
        assert_eq!(
            listed.prices,
            Prices {
                prompt: Some(30_000),
                ..Prices::default()
            }
        );
        assert_eq!(listed.context_length, Some(256_000));
    }

    #[test]
    fn a_grok_build_only_row_takes_the_seed_and_an_api_row_keeps_its_listing() {
        use super::super::{gb_listing, Listing, Registry, SourceKind};
        let api_row = ModelMeta {
            id: "grok-4.7".into(),
            context_length: Some(256_000),
            prices: Prices {
                prompt: Some(30_000),
                completion: Some(150_000),
                ..Prices::default()
            },
            ..ModelMeta::default()
        };
        let mut reg = Registry::default();
        reg.apply_refresh(
            &[
                Listing::new(SourceKind::XaiApi, vec![api_row]),
                gb_listing(&["grok-4.7".into(), "grok-4.3".into()], None),
            ],
            &[],
            1,
        );
        let live = &reg.get("grok-4.7").unwrap().meta;
        assert_eq!(
            (live.prices.prompt, live.prices.cached, live.context_length),
            (Some(30_000), None, Some(256_000))
        );
        let seeded = &reg.get("grok-4.3").unwrap().meta;
        assert_eq!(
            (
                seeded.prices.prompt,
                seeded.prices.cached,
                seeded.prices.completion,
                seeded.context_length
            ),
            (Some(12_500), Some(2_000), Some(25_000), Some(1_000_000))
        );
    }

    #[test]
    fn every_token_pays_twice_from_the_200k_step() {
        let p = seed_prices("grok-4.7").unwrap();
        assert_eq!(p.rates_at(199_999), Some((20_000, 5_000, 60_000)));
        assert_eq!(p.rates_at(200_000), Some((40_000, 10_000, 120_000)));
        assert_eq!(Prices::default().rates_at(0), None);
    }
}
