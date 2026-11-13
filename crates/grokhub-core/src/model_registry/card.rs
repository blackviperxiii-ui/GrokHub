//! The model card fill: day-one priors for the models the router weighs,
//! from the built-in profiles ([`super::builtin`], `data/model_profiles.toml`).
//! Every profiled model gets its purpose (roles), capability priors,
//! intelligence tier and, where its listing names none, its prices,
//! window, modalities and tool support, so a model is routable on its
//! merits from the first refresh. A listing wins ([`fill`] only fills
//! fields still unknown) except the fields a profile pins, and live
//! measurement replaces priors as it accrues.
//!
//! Prices are USD cents per 100M tokens, the unit xAI reports ($2 per
//! 1M is 20_000).

use super::builtin::{self, Profile};
use super::record::{route_for, ModelCard, ModelMeta, Prices, SourceKind};

/// Fill `a` from `b` when `a` is unknown, or always when the field is pinned.
fn take<T: Clone>(a: &mut Option<T>, b: &Option<T>, pinned: bool) {
    if b.is_some() && (a.is_none() || pinned) {
        a.clone_from(b);
    }
}

/// Fill a refreshed row from its profile: stamp its route from its
/// sources, then fill every still-unknown price, window, modality and
/// card field. A listing's own figures stay, except pinned ones.
pub fn fill(meta: &mut ModelMeta, sources: &[SourceKind]) {
    if meta.card.route.is_none() {
        meta.card.route = Some(route_for(&meta.id, sources));
    }
    let Some(p) = builtin::get(&meta.id) else {
        return;
    };
    fill_from(meta, &p);
}

fn fill_from(meta: &mut ModelMeta, p: &Profile) {
    // Direct xAI prices and windows come in whole from the xAI API
    // listing, or from `seed` when only Grok Build lists the model: a row
    // never mixes the two, and an API row with neither stays a
    // placeholder (a ghost). Every other provider's listing gets its gaps
    // filled.
    let direct_xai = p.provider.as_deref() == Some("xai");
    let pin = |f: &str| p.pins(f);
    let fills = |f: &str| pin(f) || !direct_xai;
    let pr = &mut meta.prices;
    if fills("prompt") {
        take(&mut pr.prompt, &p.prompt, pin("prompt"));
    }
    if fills("cached") {
        take(&mut pr.cached, &p.cached, pin("cached"));
    }
    if fills("cache_write") {
        take(&mut pr.cache_write, &p.cache_write, pin("cache_write"));
    }
    if fills("completion") {
        take(&mut pr.completion, &p.completion, pin("completion"));
    }
    if fills("prompt_long") {
        take(&mut pr.prompt_long, &p.prompt_long, pin("prompt_long"));
    }
    if fills("completion_long") {
        take(
            &mut pr.completion_long,
            &p.completion_long,
            pin("completion_long"),
        );
    }
    if fills("long_threshold") {
        take(
            &mut pr.long_context_threshold,
            &p.long_threshold,
            pin("long_threshold"),
        );
    }
    if fills("context") {
        take(&mut meta.context_length, &p.context, pin("context"));
    }
    if fills("max_output") {
        take(&mut meta.max_output, &p.max_output, pin("max_output"));
    }
    take(&mut meta.input_modalities, &p.input, pin("input"));
    take(&mut meta.output_modalities, &p.output, pin("output"));
    take(&mut meta.tool_calling, &p.tools, pin("tools"));
    take(
        &mut meta.structured_output,
        &p.structured,
        pin("structured"),
    );
    take(&mut meta.reasoning, &p.reasoning, pin("reasoning"));
    let card = p.card();
    if pin("caps") {
        meta.card.caps = card.caps;
    }
    if pin("roles") {
        meta.card.roles.clone_from(&card.roles);
    }
    meta.card.merge_from(&card);
}

/// The profile's card for a registry id, before any fill: a test and
/// diagnostics view of the profiles themselves.
pub fn bundled_card(id: &str) -> Option<ModelCard> {
    builtin::get(id).map(|p| p.card())
}

/// The profile's prices for a registry id, before any fill.
pub fn bundled_prices(id: &str) -> Option<Prices> {
    builtin::get(id).map(|p| p.prices())
}

#[cfg(test)]
mod tests {
    use super::super::record::Route;
    use super::*;

    #[test]
    fn claude_rows_carry_list_prices_and_a_cache_write_rate() {
        // Anthropic's own hyphen id, the dotted id and OpenRouter's wire
        // id all land on the same row.
        for id in [
            "anthropic/claude-sonnet-5-5",
            "anthropic/claude-sonnet-5.5",
            "openrouter/anthropic/claude-sonnet-5.5",
        ] {
            let p = bundled_prices(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(
                (p.prompt, p.cached, p.cache_write, p.completion),
                (Some(20_000), Some(1_000), Some(25_000), Some(100_000)),
                "{id}"
            );
            assert_eq!(p.cache_write_at(1_000), Some(25_000));
        }
        let haiku = bundled_prices("anthropic/claude-haiku-5.5").unwrap();
        assert_eq!(
            (haiku.prompt, haiku.cache_write, haiku.completion),
            (Some(1_000), Some(1_250), Some(5_000))
        );
        let opus = bundled_prices("anthropic/claude-opus-5.5").unwrap();
        assert_eq!(
            (opus.prompt, opus.cached, opus.completion),
            (Some(40_000), Some(2_000), Some(200_000))
        );
        // Haiku's long-prompt step: 5x past 100k tokens, cache reads too.
        assert_eq!(haiku.rates_at(100_000), Some((5_000, 500, 25_000)));
        let fable = bundled_prices("anthropic/claude-fable-5-2").unwrap();
        assert_eq!((fable.prompt, fable.cached), (Some(80_000), Some(2_000)));
        assert_eq!(bundled_prices("anthropic/claude-sonnet-4"), None);
    }

    #[test]
    fn fill_gives_an_anthropic_row_prices_a_window_and_the_anthropic_route() {
        let mut meta = ModelMeta::bare("anthropic/claude-sonnet-5.5");
        fill(&mut meta, &[SourceKind::Anthropic]);
        assert_eq!(meta.card.route, Some(Route::Anthropic));
        assert_eq!(meta.prices.completion_at(10_000), Some(100_000));
        assert_eq!(
            (meta.context_length, meta.max_output),
            (Some(1_000_000), Some(128_000))
        );
        assert_eq!(meta.card.caps.code, Some(98));
        assert!(meta.card.roles.iter().any(|r| r == "hard_code"));
        assert_eq!(meta.card.intelligence, Some(54));
        assert_eq!(meta.tool_calling, Some(true));
    }

    #[test]
    fn a_grok_build_row_gets_modalities_and_tools_but_api_prices_never_mix() {
        let mut gb = ModelMeta::bare("grok-4.7");
        fill(&mut gb, &[SourceKind::GrokBuild]);
        assert_eq!(
            gb.input_modalities.as_deref(),
            Some(&["text".to_string(), "image".to_string()][..])
        );
        assert_eq!(
            (gb.tool_calling, gb.structured_output),
            (Some(true), Some(true))
        );
        assert_eq!(
            gb.prices.cached, None,
            "xAI prices come from seed, not the card"
        );
        assert_eq!(gb.efforts, None, "efforts only ever come from a listing");
        let mut api = ModelMeta {
            id: "grok-4.7".into(),
            prices: Prices {
                prompt: Some(30_000),
                ..Prices::default()
            },
            ..ModelMeta::default()
        };
        fill(&mut api, &[SourceKind::XaiApi]);
        assert_eq!(
            api.prices,
            Prices {
                prompt: Some(30_000),
                ..Prices::default()
            }
        );
    }

    #[test]
    fn a_pinned_field_beats_the_listing() {
        let mut p = builtin::get("openrouter/deepseek/deepseek-v4.1-flash-1017").unwrap();
        p.pin = vec!["prompt".into(), "roles".into()];
        let mut meta = ModelMeta {
            id: "openrouter/deepseek/deepseek-v4.1-flash-1017".into(),
            prices: Prices {
                prompt: Some(9_999),
                completion: Some(7_777),
                ..Prices::default()
            },
            ..ModelMeta::default()
        };
        meta.card.roles = vec!["something_else".into()];
        fill_from(&mut meta, &p);
        assert_eq!(meta.prices.prompt, Some(1_500), "pinned");
        assert_eq!(
            meta.prices.completion,
            Some(7_777),
            "not pinned: the listing stays"
        );
        assert_eq!(meta.card.roles, vec!["bulk", "cheap", "long_context"]);
    }

    #[test]
    fn fill_stamps_free_and_paid_openrouter_routes_from_the_id() {
        let mut free = ModelMeta::bare("openrouter/nvidia/nemotron-3.5-super-vl:free");
        fill(&mut free, &[SourceKind::OpenAiCompatible]);
        assert_eq!(free.card.route, Some(Route::OpenRouterFree));
        assert_eq!(free.card.speed_tps, Some(96));
        assert_eq!(free.card.data_policy.trains, Some(true));
        let mut paid = ModelMeta::bare("openrouter/deepseek/deepseek-v4.1-flash-1017");
        fill(&mut paid, &[SourceKind::OpenAiCompatible]);
        assert_eq!(paid.card.route, Some(Route::OpenRouterPaid));
        assert_eq!(paid.card.hosts, Some(40));
        let mut local = ModelMeta::bare("local:gemma-4-31b");
        fill(&mut local, &[SourceKind::Local]);
        assert_eq!(local.card.route, Some(Route::Local));
        let mut xai = ModelMeta::bare("grok-4.7-0414");
        fill(&mut xai, &[SourceKind::XaiApi, SourceKind::GrokBuild]);
        assert_eq!(xai.card.route, Some(Route::Xai));
        assert!(xai.card.roles.iter().any(|r| r == "failover_first"));
    }

    #[test]
    fn fill_never_overwrites_what_a_listing_said() {
        let mut meta = ModelMeta {
            id: "openrouter/anthropic/claude-sonnet-5.5".into(),
            context_length: Some(200_000),
            prices: Prices {
                prompt: Some(21_000),
                completion: Some(99_000),
                ..Prices::default()
            },
            ..ModelMeta::default()
        };
        fill(&mut meta, &[SourceKind::OpenAiCompatible]);
        assert_eq!(
            (
                meta.prices.prompt,
                meta.prices.completion,
                meta.context_length
            ),
            (Some(21_000), Some(99_000), Some(200_000)),
            "the listing's own figures stay"
        );
        // What the listing left out still fills: the write rate and caps.
        assert_eq!(meta.prices.cache_write, Some(25_000));
        assert_eq!(meta.card.caps.code, Some(98));
    }

    #[test]
    fn the_free_roster_and_probation_rows_are_in_the_table() {
        let north = bundled_card("openrouter/cohere/north-mini-code:free").unwrap();
        assert_eq!(
            (north.data_policy.trains, north.data_policy.retains),
            (Some(false), Some(true)),
            "North Mini Code is the private free option"
        );
        assert_eq!(north.speed_tps, Some(136));
        let dots = bundled_card("openrouter/dots-studio/dots-3-note-preview:free").unwrap();
        assert_eq!(dots.expires.as_deref(), Some("2026-12-31"));
        for id in [
            "openrouter/thinkingmachines/inkling:free",
            "openrouter/poolside/laguna-s-2.1:free",
            "openrouter/nvidia/nemotron-3-super-120b-a12b:free",
            "openrouter/stealth/crouton-alpha:free",
        ] {
            assert!(bundled_card(id).unwrap().probation, "{id}");
        }
        assert_eq!(bundled_card("openrouter/openrouter/auto"), None);
        assert_eq!(bundled_card("grok-4.7-fast"), None);
    }
}
