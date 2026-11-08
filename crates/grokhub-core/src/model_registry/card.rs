//! The bundled model card table: day-one priors for the models the
//! router weighs, shipped with the app and refreshed with app updates.
//! Every known model gets its purpose (roles), capability priors and —
//! where its listing names none — its prices from here, so a model is
//! routable on its merits from the first refresh. A listing always
//! wins: [`fill`] only fills fields still unknown, and live measurement
//! replaces priors as it accrues.
//!
//! Prices are USD cents per 100M tokens, the unit xAI reports ($2 per
//! 1M is 20_000). Claude's cache rates follow Anthropic's published
//! pattern: a cache write is 1.25× the prompt rate, a read 0.1×.

use super::discover::split_provider_model;
use super::record::{
    route_for, CapabilityScores, DataPolicy, ModelCard, ModelMeta, Prices, SourceKind,
};

/// One bundled row, keyed by wire id (the id after the provider prefix).
/// A slash-less key also matches a wire id's last segment, so the Claude
/// rows serve both a direct Anthropic listing (`claude-sonnet-5.5`) and
/// OpenRouter's (`anthropic/claude-sonnet-5.5`).
struct Row {
    key: &'static str,
    roles: &'static [&'static str],
    /// chat, code, tools, reasoning, long_context, vision, audio.
    caps: [Option<u8>; 7],
    prices: Prices,
    context_length: Option<u64>,
    max_output: Option<u64>,
    data_policy: DataPolicy,
    expires: Option<&'static str>,
    hosts: Option<u32>,
    probation: bool,
    speed_tps: Option<u64>,
}

const NO_CAPS: [Option<u8>; 7] = [None, None, None, None, None, None, None];
const NO_POLICY: DataPolicy = DataPolicy {
    trains: None,
    retains: None,
    moderated: None,
};
/// NVIDIA's and Liquid's free routes may train on prompts.
const TRAINS: DataPolicy = DataPolicy {
    trains: Some(true),
    retains: None,
    moderated: None,
};
const FREE_PRICES: Prices = Prices {
    prompt: Some(0),
    cached: Some(0),
    cache_write: Some(0),
    completion: Some(0),
    prompt_long: None,
    completion_long: None,
    long_context_threshold: None,
};

const fn prices(
    prompt: u64,
    cached: Option<u64>,
    cache_write: Option<u64>,
    completion: u64,
) -> Prices {
    Prices {
        prompt: Some(prompt),
        cached,
        cache_write,
        completion: Some(completion),
        prompt_long: None,
        completion_long: None,
        long_context_threshold: None,
    }
}

const fn row(key: &'static str, roles: &'static [&'static str]) -> Row {
    Row {
        key,
        roles,
        caps: NO_CAPS,
        prices: Prices {
            prompt: None,
            cached: None,
            cache_write: None,
            completion: None,
            prompt_long: None,
            completion_long: None,
            long_context_threshold: None,
        },
        context_length: None,
        max_output: None,
        data_policy: NO_POLICY,
        expires: None,
        hosts: None,
        probation: false,
        speed_tps: None,
    }
}

const TABLE: &[Row] = &[
    // ---- xAI direct (prices and windows live in `seed`; roles and caps here)
    Row {
        roles: &["long", "agentic", "failover_first"],
        caps: [
            Some(95),
            Some(95),
            Some(95),
            Some(95),
            Some(90),
            Some(90),
            None,
        ],
        context_length: Some(500_000),
        ..row("grok-4.7", &[])
    },
    Row {
        roles: &["long", "agentic"],
        caps: [
            Some(93),
            Some(93),
            Some(93),
            Some(93),
            Some(90),
            Some(88),
            None,
        ],
        context_length: Some(500_000),
        ..row("grok-4.5", &[])
    },
    Row {
        roles: &["long", "long_context", "agentic"],
        caps: [
            Some(93),
            Some(92),
            Some(93),
            Some(92),
            Some(100),
            Some(88),
            None,
        ],
        context_length: Some(1_000_000),
        ..row("grok-4.3", &[])
    },
    // Grok on OpenRouter: a stand-in at direct prices when direct is down.
    Row {
        roles: &["stand_in"],
        caps: [
            Some(95),
            Some(95),
            Some(95),
            Some(95),
            Some(90),
            Some(90),
            None,
        ],
        prices: Prices {
            prompt: Some(20_000),
            cached: Some(5_000),
            cache_write: None,
            completion: Some(60_000),
            prompt_long: Some(40_000),
            completion_long: Some(120_000),
            long_context_threshold: Some(200_000),
        },
        context_length: Some(500_000),
        ..row("x-ai/grok-4.7", &[])
    },
    // ---- Claude (Anthropic's list prices, Nov 2026; all 1M context, 128K out)
    Row {
        roles: &["hard_code", "long"],
        caps: [
            Some(95),
            Some(98),
            Some(95),
            Some(95),
            Some(100),
            Some(95),
            None,
        ],
        prices: prices(20_000, Some(2_000), Some(25_000), 100_000),
        context_length: Some(1_000_000),
        max_output: Some(128_000),
        ..row("claude-sonnet-5.5", &[])
    },
    Row {
        roles: &["hardest", "escalation"],
        caps: [
            Some(96),
            Some(99),
            Some(96),
            Some(99),
            Some(100),
            Some(95),
            None,
        ],
        prices: prices(40_000, Some(4_000), Some(50_000), 200_000),
        context_length: Some(1_000_000),
        max_output: Some(128_000),
        ..row("claude-opus-5.5", &[])
    },
    Row {
        roles: &["cheap_fast", "tools", "vision"],
        caps: [
            Some(85),
            Some(85),
            Some(90),
            Some(80),
            Some(100),
            Some(90),
            None,
        ],
        prices: prices(1_000, Some(100), Some(1_250), 5_000),
        context_length: Some(1_000_000),
        max_output: Some(128_000),
        ..row("claude-haiku-5.5", &[])
    },
    Row {
        roles: &["escalation", "never_default"],
        caps: [
            Some(96),
            Some(97),
            Some(95),
            Some(99),
            Some(100),
            Some(95),
            None,
        ],
        prices: prices(100_000, Some(10_000), Some(125_000), 500_000),
        context_length: Some(1_000_000),
        max_output: Some(128_000),
        ..row("claude-fable-5.1", &[])
    },
    // ---- OpenRouter paid (only when the account is Paid)
    Row {
        roles: &["bulk", "cheap", "long_context"],
        caps: [
            Some(75),
            Some(80),
            Some(80),
            Some(75),
            Some(100),
            None,
            None,
        ],
        prices: prices(60, None, None, 5_000),
        context_length: Some(1_000_000),
        hosts: Some(40),
        ..row("deepseek/deepseek-v4.1-flash-1017", &[])
    },
    Row {
        roles: &["cheap_code"],
        caps: [Some(70), Some(80), Some(75), Some(70), None, None, None],
        prices: prices(280, None, None, 920),
        ..row("z-ai/glm-5.3-flash", &[])
    },
    Row {
        roles: &["cheap", "ui", "vision"],
        caps: [Some(70), Some(70), Some(70), Some(65), None, Some(85), None],
        prices: prices(750, None, None, 3_000),
        ..row("minimax/minimax-m3.1-flash", &[])
    },
    Row {
        roles: &["classifier", "cheap"],
        caps: [Some(60), Some(55), Some(65), Some(55), None, None, None],
        prices: prices(1_000, None, None, 5_000),
        ..row("openai/gpt-6-luna", &[])
    },
    Row {
        roles: &["code", "agents", "mid_price"],
        caps: [
            Some(85),
            Some(88),
            Some(88),
            Some(85),
            Some(90),
            Some(90),
            None,
        ],
        prices: prices(7_500, None, None, 37_500),
        ..row("google/gemini-3.8-flash", &[])
    },
    Row {
        roles: &["backup", "low_latency"],
        caps: [Some(75), Some(75), Some(75), Some(70), None, Some(80), None],
        prices: prices(3_000, None, None, 25_000),
        ..row("google/gemini-3.5-flash-lite", &[])
    },
    Row {
        roles: &["long_code"],
        caps: [Some(85), Some(93), Some(90), Some(90), Some(95), None, None],
        prices: prices(20_000, None, None, 100_000),
        ..row("moonshotai/kimi-k3.1-pro", &[])
    },
    Row {
        roles: &["documents", "vision", "long_context"],
        caps: [
            Some(90),
            Some(85),
            Some(85),
            Some(90),
            Some(100),
            Some(97),
            None,
        ],
        prices: prices(20_000, None, None, 100_000),
        ..row("google/gemini-4-argon", &[])
    },
    Row {
        roles: &["frontier"],
        caps: [Some(95), Some(95), Some(90), Some(95), None, Some(90), None],
        prices: prices(20_000, None, None, 100_000),
        ..row("openai/gpt-6.1-sol", &[])
    },
    // ---- OpenRouter free (one host each; quota, never money)
    Row {
        roles: &[
            "free_primary",
            "short_chat",
            "vision",
            "screenshots",
            "tools",
        ],
        caps: [
            Some(78),
            Some(75),
            Some(85),
            Some(75),
            Some(100),
            Some(92),
            None,
        ],
        prices: FREE_PRICES,
        context_length: Some(1_000_000),
        data_policy: TRAINS,
        hosts: Some(1),
        speed_tps: Some(96),
        ..row("nvidia/nemotron-3.5-super-vl:free", &[])
    },
    Row {
        roles: &["free_code", "private"],
        caps: [Some(70), Some(80), Some(75), Some(70), Some(85), None, None],
        prices: FREE_PRICES,
        context_length: Some(256_000),
        data_policy: DataPolicy {
            trains: Some(false),
            retains: Some(true),
            moderated: None,
        },
        hosts: Some(1),
        speed_tps: Some(136),
        ..row("cohere/north-mini-code:free", &[])
    },
    Row {
        roles: &["fast_chat", "classification"],
        caps: [Some(65), Some(60), Some(65), Some(40), None, None, None],
        prices: FREE_PRICES,
        data_policy: TRAINS,
        hosts: Some(1),
        ..row("nvidia/nemotron-3.5-lightning:free", &[])
    },
    Row {
        roles: &["perception", "screenshots", "audio", "clips"],
        caps: [
            Some(55),
            Some(45),
            Some(55),
            Some(35),
            None,
            Some(80),
            Some(85),
        ],
        prices: FREE_PRICES,
        data_policy: TRAINS,
        hosts: Some(1),
        ..row("nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free", &[])
    },
    Row {
        roles: &["free_reasoning", "background", "long_free"],
        caps: [Some(85), Some(85), Some(80), Some(92), None, None, None],
        prices: FREE_PRICES,
        data_policy: TRAINS,
        hosts: Some(1),
        ..row("nvidia/nemotron-3-ultra-550b-a55b:free", &[])
    },
    Row {
        roles: &["probation"],
        caps: [
            Some(80),
            Some(78),
            Some(78),
            Some(82),
            Some(100),
            None,
            None,
        ],
        prices: FREE_PRICES,
        context_length: Some(1_000_000),
        data_policy: TRAINS,
        hosts: Some(1),
        probation: true,
        ..row("nvidia/nemotron-3-super-120b-a12b:free", &[])
    },
    Row {
        roles: &["probation"],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("thinkingmachines/inkling:free", &[])
    },
    Row {
        roles: &["probation"],
        caps: [None, Some(78), Some(80), None, None, None, None],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("poolside/laguna-s-2.1:free", &[])
    },
    Row {
        roles: &["code_fallback", "temporary"],
        caps: [Some(60), Some(65), Some(60), Some(55), None, None, None],
        prices: FREE_PRICES,
        expires: Some("2026-12-31"),
        hosts: Some(1),
        ..row("dots-studio/dots-3-note-preview:free", &[])
    },
    Row {
        roles: &["free_code", "unstable"],
        caps: [Some(60), Some(65), Some(60), Some(55), None, None, None],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("recursive/flint-v1.0:free", &[])
    },
    Row {
        roles: &["probation"],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("stealth/crouton-alpha:free", &[])
    },
    Row {
        roles: &["probation"],
        prices: FREE_PRICES,
        data_policy: TRAINS,
        hosts: Some(1),
        probation: true,
        ..row("liquid/lfm-3-1.7b:free", &[])
    },
    Row {
        roles: &["probation"],
        prices: FREE_PRICES,
        data_policy: TRAINS,
        expires: Some("2026-11-13"),
        hosts: Some(1),
        probation: true,
        ..row("liquid/lfm-2.5-2.6b:free", &[])
    },
    Row {
        roles: &["avoid"],
        caps: [Some(30), Some(25), Some(25), Some(25), None, None, None],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("xingchen-agi/xing4.0-29b-a4b:free", &[])
    },
    // Both Gemma free endpoints answered nothing but 429s in testing:
    // the priors are for the same models run locally instead.
    Row {
        roles: &["run_local"],
        caps: [Some(70), Some(65), Some(65), Some(65), None, Some(70), None],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("google/gemma-4-31b-it:free", &[])
    },
    Row {
        roles: &["run_local"],
        caps: [Some(68), Some(63), Some(63), Some(63), None, Some(68), None],
        prices: FREE_PRICES,
        hosts: Some(1),
        probation: true,
        ..row("google/gemma-4-26b-a4b-it:free", &[])
    },
];

/// The bundled row for a registry id: matched on its wire id, on the
/// wire id's last segment for slash-less keys (the Claude rows), or on
/// the id stem for xAI's dated and `-latest` forms.
fn bundled(id: &str) -> Option<&'static Row> {
    let wire = split_provider_model(id).map_or(id, |(_, m)| m);
    if let Some(r) = TABLE.iter().find(|r| r.key == wire) {
        return Some(r);
    }
    let last = wire.rsplit('/').next().unwrap_or(wire);
    let stem = super::record::model_id_stem(wire);
    TABLE
        .iter()
        .find(|r| (!r.key.contains('/') && (r.key == last || r.key == stem)) || r.key == stem)
}

impl Row {
    fn card(&self) -> ModelCard {
        ModelCard {
            route: None,
            caps: CapabilityScores {
                chat: self.caps[0],
                code: self.caps[1],
                tools: self.caps[2],
                reasoning: self.caps[3],
                long_context: self.caps[4],
                vision: self.caps[5],
                audio: self.caps[6],
            },
            roles: self.roles.iter().map(|s| s.to_string()).collect(),
            data_policy: self.data_policy,
            expires: self.expires.map(str::to_string),
            hosts: self.hosts,
            probation: self.probation,
            speed_tps: self.speed_tps,
            ttft_ms: None,
        }
    }
}

/// Fill a refreshed row from the bundled table: stamp its route from its
/// sources, then fill every still-unknown price, window and card field
/// from its row. A listing's own figures always stay.
pub fn fill(meta: &mut ModelMeta, sources: &[SourceKind]) {
    if meta.card.route.is_none() {
        meta.card.route = Some(route_for(&meta.id, sources));
    }
    let Some(r) = bundled(&meta.id) else {
        return;
    };
    fn take<T: Clone>(a: &mut Option<T>, b: &Option<T>) {
        if a.is_none() {
            a.clone_from(b);
        }
    }
    take(&mut meta.prices.prompt, &r.prices.prompt);
    take(&mut meta.prices.cached, &r.prices.cached);
    take(&mut meta.prices.cache_write, &r.prices.cache_write);
    take(&mut meta.prices.completion, &r.prices.completion);
    take(&mut meta.prices.prompt_long, &r.prices.prompt_long);
    take(&mut meta.prices.completion_long, &r.prices.completion_long);
    take(
        &mut meta.prices.long_context_threshold,
        &r.prices.long_context_threshold,
    );
    take(&mut meta.context_length, &r.context_length);
    take(&mut meta.max_output, &r.max_output);
    meta.card.merge_from(&r.card());
}

/// The bundled card for a registry id, before any fill: a test and
/// diagnostics view of the table itself.
pub fn bundled_card(id: &str) -> Option<ModelCard> {
    bundled(id).map(Row::card)
}

/// The bundled prices for a registry id, before any fill.
pub fn bundled_prices(id: &str) -> Option<Prices> {
    bundled(id).map(|r| r.prices.clone())
}

#[cfg(test)]
mod tests {
    use super::super::record::Route;
    use super::*;

    #[test]
    fn claude_rows_carry_list_prices_and_a_cache_write_rate() {
        // Direct Anthropic listing id and OpenRouter's wire id both land
        // on the same row.
        for id in [
            "anthropic/claude-sonnet-5.5",
            "openrouter/anthropic/claude-sonnet-5.5",
        ] {
            let p = bundled_prices(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(
                (p.prompt, p.cached, p.cache_write, p.completion),
                (Some(20_000), Some(2_000), Some(25_000), Some(100_000)),
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
            (opus.prompt, opus.completion),
            (Some(40_000), Some(200_000))
        );
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
