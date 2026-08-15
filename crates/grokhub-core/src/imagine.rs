use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_IMAGINE_MODEL: &str = "grok-2-image";

/// Imagine never shares the chat model. Only an explicit *image* model wins.
pub fn dedicated_imagine_model(user: &str) -> String {
    let u = user.trim();
    if u.contains("image") {
        u.to_string()
    } else {
        DEFAULT_IMAGINE_MODEL.to_string()
    }
}

pub fn imagine_request_body(prompt: &str, model: &str) -> Value {
    json!({
        "model": dedicated_imagine_model(model),
        "prompt": prompt,
        "n": 1,
        "response_format": "b64_json",
    })
}

pub fn imagine_slug(prompt: &str) -> String {
    let s: String = prompt
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "imagine".into()
    } else {
        s
    }
}

pub fn parse_imagine_url(body: &Value) -> Option<String> {
    let data = body.get("data")?.as_array()?.first()?;
    data.get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.get("b64_json")
                .and_then(|v| v.as_str())
                .map(|s| format!("data:image/png;base64,{s}"))
        })
}

/// Assistant verb that kicks Imagine. Distinct from `IMAGINE: <url>` receipts.
pub fn extract_imagine_prompt(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("IMAGINE_PROMPT:") else {
            continue;
        };
        let p = rest.trim();
        if !p.is_empty() {
            return Some(p.chars().take(400).collect());
        }
    }
    None
}

/// Twenty live covers. Oldest leaves first.
pub const WALL_GIF_MAX: usize = 20;
/// A new cover every few hours.
pub const WALL_GIF_EVERY_MS: u64 = 3 * 60 * 60 * 1000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagineWall {
    #[serde(default)]
    pub last_ms: u64,
    #[serde(default)]
    pub gifs: Vec<WallGif>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallGif {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub created_ms: u64,
    pub path_a: String,
    pub path_b: String,
    pub tall: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallSeed {
    pub title: &'static str,
    pub prompt: &'static str,
    pub prompt_b: &'static str,
    pub tall: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallSlot {
    Stock(usize),
    Gif(usize),
}

/// Cabin-real stills the night can paint. No video, no faces, no photo-edit verbs.
pub const WALL_SEEDS: &[WallSeed] = &[
    WallSeed {
        title: "Ember night",
        prompt: "still of dying embers in a dark timber cabin stove, no people, no text",
        prompt_b: "still of the same cabin stove a breath later, closer on the grate, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Snow porch",
        prompt: "still of a cabin porch at night, snow on the rail, one lantern, no people, no text",
        prompt_b: "still of the same snow porch, wider, pines beyond the rail, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Kettle steam",
        prompt: "still of a black kettle on a wood stove, faint steam, dark cabin, no people, no text",
        prompt_b: "still of the same kettle, closer, steam catching lamplight, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Bound ledger",
        prompt: "still of a bound ledger on a worn desk, cabin lamp, no people, no readable text",
        prompt_b: "still of the same ledger, pages half turned, no people, no readable text",
        tall: false,
    },
    WallSeed {
        title: "Frost pane",
        prompt: "still of frost on a cabin window at dawn, dark room, no people, no text",
        prompt_b: "still of the same frosted pane, closer crystals, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Tool wall",
        prompt: "still of hand tools hung on dark cabin wood, warm lamp, no people, no text",
        prompt_b: "still of the same tool wall, tighter crop, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Creek ice",
        prompt: "still of a frozen creek below pines at night, no people, no text",
        prompt_b: "still of the same creek, closer on the ice edge, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Oil lamp",
        prompt: "still of an oil lamp on a cabin table, dark room, no people, no text",
        prompt_b: "still of the same lamp, glass glowing, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Split wood",
        prompt: "still of split firewood stacked by a cabin door, night, no people, no text",
        prompt_b: "still of the same woodpile, closer bark and frost, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Empty mug",
        prompt: "still of an empty enamel mug on a windowsill, cabin night, no people, no text",
        prompt_b: "still of the same mug, frost on the pane behind it, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Ridge wind",
        prompt: "still of a wind-cut pine ridge above a dark valley, no people, no text",
        prompt_b: "still of the same ridge, clouds moving in, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Wool blanket",
        prompt: "still of a folded wool blanket on a wooden chair, cabin lamp, no people, no text",
        prompt_b: "still of the same chair, blanket slightly shifted, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Host glow",
        prompt: "still of a Linux workstation in a dark cabin, monitor glow, no people, no faces, no text",
        prompt_b: "still of the same desk, closer on the dark wood and keys, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Night path",
        prompt: "still of a snow path to a cabin, one window lit, no people, no text",
        prompt_b: "still of the same path, a few steps closer, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Iron latch",
        prompt: "still of an iron latch on a heavy cabin door, lamplight, no people, no text",
        prompt_b: "still of the same latch, closer metal grain, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Quiet shelf",
        prompt: "still of a cabin shelf of blank notebooks, warm lamp, no people, no readable text",
        prompt_b: "still of the same shelf, one book pulled forward, no people, no readable text",
        tall: false,
    },
    WallSeed {
        title: "Ash bucket",
        prompt: "still of an ash bucket beside a wood stove, dark cabin, no people, no text",
        prompt_b: "still of the same bucket, embers reflecting, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Pine table",
        prompt: "still of a bare pine table, one lamp, empty cabin, no people, no text",
        prompt_b: "still of the same table, wider, chairs in shadow, no people, no text",
        tall: false,
    },
];

pub fn wall_due(last_ms: u64, now_ms: u64, interval_ms: u64) -> bool {
    if last_ms == 0 {
        return true;
    }
    now_ms.saturating_sub(last_ms) >= interval_ms
}

pub fn wall_can_paint(
    has_key: bool,
    wall_on: bool,
    wall_busy: bool,
    running: bool,
    quiet: bool,
    last_ms: u64,
    now_ms: u64,
) -> bool {
    if !has_key || !wall_on || wall_busy || running {
        return false;
    }
    if last_ms == 0 {
        return true;
    }
    if quiet {
        return false;
    }
    wall_due(last_ms, now_ms, WALL_GIF_EVERY_MS)
}

pub fn wall_evict(mut gifs: Vec<WallGif>, max: usize) -> (Vec<WallGif>, Vec<WallGif>) {
    gifs.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then_with(|| a.id.cmp(&b.id)));
    if gifs.len() <= max {
        return (gifs, Vec::new());
    }
    let drop_n = gifs.len() - max;
    let evicted = gifs.drain(..drop_n).collect();
    (gifs, evicted)
}

pub fn pick_fresh_seed(roll: u64, taken: &[&str]) -> &'static WallSeed {
    let n = WALL_SEEDS.len().max(1);
    for i in 0..n {
        let s = &WALL_SEEDS[((roll as usize) + i) % n];
        if !taken.iter().any(|t| t.eq_ignore_ascii_case(s.title)) {
            return s;
        }
    }
    &WALL_SEEDS[(roll as usize) % n]
}

fn lcg(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1)
}

pub fn curate_wall(stock_n: usize, gif_n: usize, seed: u64) -> Vec<WallSlot> {
    let mut slots: Vec<WallSlot> = (0..stock_n).map(WallSlot::Stock).collect();
    let mut order: Vec<usize> = (0..gif_n).collect();
    let mut s = seed | 1;
    if gif_n > 1 {
        for i in (1..order.len()).rev() {
            s = lcg(s);
            let j = (s as usize) % (i + 1);
            order.swap(i, j);
        }
    }
    for (k, gi) in order.into_iter().enumerate() {
        s = lcg(s);
        let at = (s as usize) % (stock_n + k + 1);
        slots.insert(at.min(slots.len()), WallSlot::Gif(gi));
    }
    slots
}

pub fn wall_curate_seed(gifs: &[WallGif]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for g in gifs {
        for b in g.id.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= g.created_ms;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn imagine_dest(project: Option<&str>) -> String {
    match project.filter(|s| !s.is_empty()) {
        Some(p) => format!("{p}/imagine"),
        None => "GrokHub-Work/imagine".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_url() {
        assert_eq!(dedicated_imagine_model("grok-3-mini-fast"), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-2-image-1212"), "grok-2-image-1212");
        let b = imagine_request_body("a cabin at night", "grok-3-mini-fast");
        assert_eq!(b["model"], DEFAULT_IMAGINE_MODEL);
        assert_eq!(b["response_format"], "b64_json");
        assert_eq!(dedicated_imagine_model(""), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-imagine"), DEFAULT_IMAGINE_MODEL);
        let reply = json!({ "data": [{ "url": "https://img/x.png" }] });
        assert_eq!(parse_imagine_url(&reply).as_deref(), Some("https://img/x.png"));
        assert_eq!(imagine_dest(None), "GrokHub-Work/imagine");
        assert_eq!(
            extract_imagine_prompt("ok\nIMAGINE_PROMPT: a cabin at night\n").as_deref(),
            Some("a cabin at night")
        );
        assert!(extract_imagine_prompt("IMAGINE: https://img/x.png").is_none());
    }

    fn gif(id: &str, created_ms: u64) -> WallGif {
        WallGif {
            id: id.into(),
            title: id.into(),
            prompt: format!("still of {id}, no people, no text"),
            created_ms,
            path_a: format!("{id}_a.jpg"),
            path_b: format!("{id}_b.jpg"),
            tall: false,
        }
    }

    #[test]
    fn wall_paints_every_few_hours() {
        assert_eq!(WALL_GIF_MAX, 20);
        assert_eq!(WALL_GIF_EVERY_MS, 3 * 60 * 60 * 1000);
        assert!(wall_due(0, 1, WALL_GIF_EVERY_MS));
        assert!(!wall_due(1_000, 1_000 + WALL_GIF_EVERY_MS - 1, WALL_GIF_EVERY_MS));
        assert!(wall_due(1_000, 1_000 + WALL_GIF_EVERY_MS, WALL_GIF_EVERY_MS));
        assert!(!wall_can_paint(false, true, false, false, false, 0, 10_000));
        assert!(!wall_can_paint(true, false, false, false, false, 0, 10_000));
        assert!(!wall_can_paint(true, true, true, false, false, 0, 10_000));
        assert!(!wall_can_paint(true, true, false, true, false, 0, 10_000));
        assert!(wall_can_paint(true, true, false, false, true, 0, 10_000));
        assert!(wall_can_paint(true, true, false, false, false, 0, 10_000));
        assert!(!wall_can_paint(
            true,
            true,
            false,
            false,
            true,
            1_000,
            1_000 + WALL_GIF_EVERY_MS
        ));
        assert!(!wall_can_paint(
            true,
            true,
            false,
            false,
            false,
            1_000,
            1_000 + WALL_GIF_EVERY_MS - 1
        ));
    }

    #[test]
    fn wall_evicts_oldest_first() {
        let gifs: Vec<WallGif> = (0..22).map(|i| gif(&format!("g{i}"), 100 + i)).collect();
        let (kept, evicted) = wall_evict(gifs, WALL_GIF_MAX);
        assert_eq!(kept.len(), 20);
        assert_eq!(evicted.len(), 2);
        assert_eq!(evicted[0].id, "g0");
        assert_eq!(evicted[1].id, "g1");
        assert_eq!(kept[0].id, "g2");
        assert_eq!(kept.last().unwrap().id, "g21");
        let five: Vec<WallGif> = (0..5).map(|i| gif(&format!("k{i}"), i)).collect();
        let (kept, evicted) = wall_evict(five, WALL_GIF_MAX);
        assert_eq!(kept.len(), 5);
        assert!(evicted.is_empty());
    }

    #[test]
    fn wall_curation_is_random_and_stable() {
        assert!(WALL_SEEDS.len() >= 16);
        for s in WALL_SEEDS {
            let blob = format!("{} {} {}", s.title, s.prompt, s.prompt_b).to_ascii_lowercase();
            assert!(blob.contains("still") || blob.contains("cabin") || blob.contains("desk"));
            assert!(blob.contains("no people"));
            assert!(!blob.contains("video"));
            assert!(!blob.contains("photo edit"));
        }
        let taken = ["Ember night"];
        let a = pick_fresh_seed(0, &taken);
        assert_ne!(a.title, "Ember night");
        let slots_a = curate_wall(9, 4, 42);
        let slots_b = curate_wall(9, 4, 42);
        assert_eq!(slots_a, slots_b);
        assert_eq!(slots_a.len(), 13);
        assert_eq!(
            slots_a.iter().filter(|s| matches!(s, WallSlot::Stock(_))).count(),
            9
        );
        assert_eq!(
            slots_a.iter().filter(|s| matches!(s, WallSlot::Gif(_))).count(),
            4
        );
        let slots_c = curate_wall(9, 4, 99);
        assert_ne!(slots_a, slots_c);
        let gifs = vec![gif("alpha", 1), gif("beta", 2)];
        assert_eq!(wall_curate_seed(&gifs), wall_curate_seed(&gifs));
        assert_ne!(wall_curate_seed(&gifs), wall_curate_seed(&[gif("alpha", 1)]));
    }
}
