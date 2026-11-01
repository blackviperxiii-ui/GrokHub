//! Brain 1's local sorter: when the plain rules can't tell what an ask is, a
//! small in-process scorer guesses its intent before anything calls a model.
//!
//! The ask becomes hashed word, word-pair and concept features (1024 buckets,
//! weighted by how rare each bucket is across the examples below). Each intent
//! has a centroid built once from short example asks; the ask's nearest
//! centroid is the guess, and how far it beats the runner-up is the confidence.
//! No model, no network, no files: the tables are compiled in.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::brief::Intent;

const DIMS: usize = 1024;
/// A concept hit counts this many times a plain word.
const CONCEPT_WEIGHT: f32 = 2.0;
/// A lead of this much cosine over the runner-up is full confidence.
const MARGIN_FULL: f32 = 0.07;
/// A best cosine under this scales the confidence down: an ask that matches
/// little of anything is a weak guess even when one intent edges ahead.
const TOP_FULL: f32 = 0.15;
/// The nearest example names the domain only when it is at least this close.
const DOMAIN_NEAR: f32 = 0.3;

/// The scorer's guess for one ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sorted {
    pub intent: Intent,
    /// 0..=100: how far the best intent leads the runner-up.
    pub confidence: u8,
    /// The nearest example's domain, or `general` when none is close.
    pub domain: &'static str,
}

/// Short asks the rules leave unsorted, grouped by intent and domain.
/// Background is never guessed: the rules always sort a background run.
pub(super) const EXEMPLARS: &[(Intent, &str, &[&str])] = &[
    (
        Intent::Chat,
        "general",
        &[
            "just got back from the gym",
            "my cat knocked my coffee over again lol",
            "that movie last night was wild",
            "pizza tonight i think",
            "honestly not much going on",
            "weekend finally",
            "just chilling tonight",
            "nah just chill",
            "good morning to you too",
            "long story but it worked out",
            "i finally finished that book",
            "my sister visits this weekend",
            "rainy day here, staying in",
            "same here honestly",
            "that's fair",
            "i had the best tacos for lunch",
            "sounds good to me",
            "nothing much, just hanging around",
            "ok cool, talk later",
            "my team won last night",
        ],
    ),
    (
        Intent::Question,
        "general",
        &[
            "the difference between a virus and bacteria",
            "wondering if bananas are safe for dogs",
            "curious whether coffee dehydrates you",
            "meaning of the word ephemeral",
            "the boiling point of water at high altitude",
            "any idea if the store is open on sunday",
            "the best way to learn piano as an adult",
            "tell me the population of canada",
            "the reason the sky looks blue",
            "is there a rule for when to use whom",
            "the time zone for tokyo",
            "quick question about how mortgages work",
            "the capital of australia",
            "the definition of inflation",
            "not sure if eggs need to be refrigerated",
            "i wonder why cats purr",
            "the rules of cricket in simple terms",
            "the history behind halloween",
            "the healthiest kind of rice",
            "i keep wondering if pluto is a planet",
        ],
    ),
    (
        Intent::Task,
        "files",
        &[
            "the spreadsheet from yesterday",
            "need the budget spreadsheet sorted by month",
            "a backup of my photos onto the external drive",
            "zip up the vacation photos",
            "the pdf from last week, merge it with the invoice",
            "clear out my old screenshots",
            "empty the recycle bin",
            "get rid of the duplicate songs",
        ],
    ),
    (
        Intent::Task,
        "general",
        &[
            "a reminder for my mom's birthday next week",
            "a timer for 20 minutes",
            "grocery list for the week",
            "email to the landlord about the lease",
            "need a cover letter for the marketing job",
            "my resume needs a fresh summary section",
            "reply to the group chat saying i'll be late",
            "a packing list for the camping trip",
            "put dentist on my calendar for friday at 3",
            "translate this paragraph into spanish",
            "summarize the meeting notes into bullet points",
            "need a table of my monthly expenses",
            "a shopping list from this recipe",
            "book a table for two at eight",
        ],
    ),
    (Intent::Task, "desktop", &["dark mode on in the settings"]),
    (
        Intent::Repair,
        "general",
        &[
            "my headphones keep cutting out",
            "the wifi drops every few minutes",
            "sound is gone on my laptop",
            "the printer just blinks orange",
            "keyboard typing double letters",
            "bluetooth keeps disconnecting",
            "my mic isn't picking up anything",
            "the internet is super slow today",
            "my phone won't charge past eighty percent",
            "the mouse keeps lagging and jumping",
            "the fan is loud and the laptop gets hot",
            "my webcam shows a black square",
            "emails stopped syncing on my phone",
            "the video keeps buffering forever",
            "the second monitor isn't detected anymore",
            "audio is out of sync with the video",
            "my login keeps getting rejected",
            "the update is stuck at forty percent",
        ],
    ),
    (
        Intent::Repair,
        "desktop",
        &[
            "the screen goes black randomly",
            "the app closes as soon as it opens",
        ],
    ),
    (
        Intent::Creative,
        "media",
        &[
            "a limerick about my cat",
            "rap verse about mondays",
            "birthday card message for my sister",
            "a toast for my brother's wedding",
            "bedtime tale about a brave little dragon",
            "a catchy tagline for my bakery",
            "fantasy character names for my campaign",
            "a funny caption for my vacation photo",
            "a short speech for my dad's retirement party",
            "some rhymes about pizza",
            "a sonnet for valentine's day",
            "cute names for a golden retriever puppy",
            "a scary campfire tale",
            "a love letter in pirate speak",
            "an opening line for my novel",
            "a jingle for a lemonade stand",
            "a riddle for a kid's treasure hunt",
            "a wedding vow that makes people cry",
            "a fun bio for my dating profile",
            "band name ideas for a garage rock group",
        ],
    ),
    (
        Intent::Research,
        "web",
        &[
            "pros and cons of heat pumps vs furnaces",
            "evidence on intermittent fasting",
            "a deep dive into the history of the printing press",
            "the best budget e-readers right now with reviews",
            "overview of current electric car battery tech",
            "background reading on the french revolution",
            "what experts say about standing desks",
            "a summary of studies on screen time for kids",
            "the top rated robot vacuums this year",
            "differences between the three biggest cloud providers, with citations",
            "market trends for home solar panels",
            "a report on remote work productivity studies",
            "reviews of the new flagship phones side by side",
            "the state of nuclear fusion as of this year",
            "a breakdown of interest rate changes over the past decade",
            "which mattress brands have the best reviews",
            "the science behind cold plunges, with studies",
            "recent findings on microplastics in food",
            "a rundown of the best note taking apps with ratings",
            "an analysis of housing prices across major cities",
        ],
    ),
    (Intent::Repair, "web", &["websites take ages to open"]),
];

/// Words that mean the same kind of thing share one extra feature, so an ask
/// can match an example without sharing its exact words ("earbuds" and
/// "headphones", "silent" and "drops"). Each list is space-separated.
const CONCEPTS: &[(&str, &str)] = &[
    (
        "device",
        "headphone headphones earbud earbuds airpods speaker speakers mic microphone \
         webcam camera mouse keyboard monitor display screen printer router modem wifi \
         bluetooth internet laptop phone tablet tv console controller headset charger \
         battery usb hdmi ssd audio sound video videos volume signal connection network \
         cable adapter fan trackpad touchpad wireless",
    ),
    (
        "symptom",
        "silent muted mute black blank slow sluggish lag lags laggy drop drops dropping \
         disconnect disconnects disconnecting disconnected loses losing lost flicker \
         flickers flickering flash flashes flashing blink blinks blinking dead overheat \
         overheating hot burning loud noisy buffering pause pauses stutter glitch glitchy \
         glitches nothing garbled static distorted unresponsive missing gone fuzzy blurry \
         dim forever randomly anymore weird keeps load loading",
    ),
    (
        "piece",
        "toast speech caption motto tagline verse bars rhyme rhymes limerick sonnet vow \
         vows note card names bio jingle riddle tale rap ode pun puns greeting heartfelt \
         witty funny punchy catchy cute sweet clever fun character characters plot novel \
         chapter lines words nickname nicknames",
    ),
    (
        "evidence",
        "studies study evidence trials trial clinical reviews reviewed review ratings \
         rated references cited citations experts expert survey tradeoffs pros cons vs \
         versus analysis data statistics stats findings dive overview report papers \
         benchmarks rankings alternatives options sources breakdown",
    ),
    (
        "schedule",
        "calendar remind reminder timer countdown alarm monday tuesday wednesday thursday \
         friday saturday sunday tomorrow appointment meeting deadline minutes pm am",
    ),
    (
        "doc",
        "email spreadsheet sheet letter list pdf resume cv document doc invoice table \
         notes summary message slides presentation form template draft french spanish \
         german translate sorted paragraph todo",
    ),
    (
        "purge",
        "wipe erase clear empty purge trash junk shred rid reset duplicates duplicate \
         leftover clutter",
    ),
    (
        "fact",
        "meaning mean definition reason difference whether tallest biggest largest \
         smallest fastest speed distance capital population hours safe healthy fact true \
         actually curious wondering wonder or",
    ),
    (
        "life",
        "dinner lunch breakfast morning weekend today day store gym run dog cat brother \
         sister mom dad friend show movie weather finally just pretty chill back great \
         nice lol honestly fair same",
    ),
];

/// A sparse feature vector: (bucket, weight) pairs sorted by bucket.
type Sparse = Vec<(usize, f32)>;

/// A word in its plain form: "disconnects" and "disconnecting" share one.
fn stem(w: &str) -> &str {
    let w = w.strip_suffix("'s").unwrap_or(w);
    for suffix in ["ing", "ed", "es", "s"] {
        if w.chars().count() > suffix.len() + 2 {
            if let Some(stem) = w.strip_suffix(suffix) {
                return stem;
            }
        }
    }
    w
}

/// FNV-1a over a tag byte and the parts, folded into a bucket.
fn bucket(tag: u8, parts: &[&str]) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    eat(tag);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            eat(b' ');
        }
        part.bytes().for_each(&mut eat);
    }
    (h % DIMS as u64) as usize
}

/// The ask's raw feature counts, sorted by bucket with repeats summed.
fn features(text: &str, concepts: &HashMap<&'static str, Vec<usize>>) -> Sparse {
    let lower = text.to_lowercase();
    let raw = super::brief::words(&lower);
    let stems: Vec<&str> = raw.iter().map(|w| stem(w)).collect();
    let mut out: Sparse = Vec::with_capacity(stems.len() * 3);
    out.extend(stems.iter().map(|w| (bucket(b'u', &[w]), 1.0)));
    out.extend(stems.windows(2).map(|p| (bucket(b'b', p), 1.0)));
    for w in &raw {
        if let Some(hits) = concepts.get(w) {
            out.extend(hits.iter().map(|&b| (b, CONCEPT_WEIGHT)));
        }
    }
    out.sort_unstable_by_key(|&(b, _)| b);
    out.dedup_by(|next, kept| {
        let same = next.0 == kept.0;
        if same {
            kept.1 += next.1;
        }
        same
    });
    out
}

fn normalize(v: &mut [(usize, f32)]) {
    let n = v.iter().map(|(_, x)| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|(_, x)| *x /= n);
    }
}

/// Dot product of two bucket-sorted sparse vectors.
fn dot(a: &[(usize, f32)], b: &[(usize, f32)]) -> f32 {
    let (mut i, mut j, mut sum) = (0, 0, 0.0);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                sum += a[i].1 * b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    sum
}

struct Model {
    concepts: HashMap<&'static str, Vec<usize>>,
    /// Bucket weight; 0 for a bucket no example uses.
    idf: Vec<f32>,
    /// One unit-length centroid per intent, dense.
    centroids: Vec<(Intent, Vec<f32>)>,
    /// Each example's weighted unit vector, for the domain.
    examples: Vec<(Intent, &'static str, Sparse)>,
}

impl Model {
    fn build() -> Self {
        let mut concepts: HashMap<&'static str, Vec<usize>> = HashMap::new();
        for (name, list) in CONCEPTS {
            let b = bucket(b'c', &[name]);
            for w in list.split_whitespace() {
                concepts.entry(w).or_default().push(b);
            }
        }
        let raw: Vec<_> = EXEMPLARS
            .iter()
            .flat_map(|(intent, domain, texts)| texts.iter().map(move |t| (*intent, *domain, t)))
            .map(|(intent, domain, text)| (intent, domain, features(text, &concepts)))
            .collect();
        let mut df = vec![0u32; DIMS];
        for (_, _, f) in &raw {
            f.iter().for_each(|&(b, _)| df[b] += 1);
        }
        let n = raw.len() as f32;
        let idf: Vec<f32> = df
            .iter()
            .map(|&d| match d {
                0 => 0.0,
                d => ((n + 1.0) / (d as f32 + 1.0)).ln() + 1.0,
            })
            .collect();
        let mut model = Model {
            concepts,
            idf,
            centroids: Vec::new(),
            examples: Vec::new(),
        };
        for (intent, domain, f) in raw {
            let v = model.weigh(f);
            let at = match model.centroids.iter().position(|(i, _)| *i == intent) {
                Some(at) => at,
                None => {
                    model.centroids.push((intent, vec![0.0; DIMS]));
                    model.centroids.len() - 1
                }
            };
            v.iter().for_each(|&(b, x)| model.centroids[at].1[b] += x);
            model.examples.push((intent, domain, v));
        }
        for (_, c) in &mut model.centroids {
            let n = c.iter().map(|x| x * x).sum::<f32>().sqrt();
            c.iter_mut().for_each(|x| *x /= n);
        }
        model
    }

    /// Raw counts to a unit vector in rare-word weights; unknown buckets drop out.
    fn weigh(&self, mut f: Sparse) -> Sparse {
        f.retain_mut(|(b, x)| {
            *x *= self.idf[*b];
            *x > 0.0
        });
        normalize(&mut f);
        f
    }
}

fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(Model::build)
}

/// The scorer's guess for an ask, or `None` when nothing in it matches any example.
pub(super) fn sort(text: &str) -> Option<Sorted> {
    let m = model();
    let q = m.weigh(features(text, &m.concepts));
    if q.is_empty() {
        return None;
    }
    let mut scores: Vec<(f32, Intent)> = m
        .centroids
        .iter()
        .map(|(intent, c)| (q.iter().map(|&(b, x)| x * c[b]).sum(), *intent))
        .collect();
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (top, intent) = scores[0];
    let margin = top - scores.get(1).map_or(0.0, |s| s.0);
    let sure = (margin / MARGIN_FULL).min(1.0) * (top / TOP_FULL).min(1.0);
    let domain = m
        .examples
        .iter()
        .filter(|(i, _, _)| *i == intent)
        .map(|(_, d, v)| (dot(&q, v), *d))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|(near, _)| *near >= DOMAIN_NEAR)
        .map_or("general", |(_, d)| d);
    Some(Sorted {
        intent,
        confidence: (sure.clamp(0.0, 1.0) * 100.0).round() as u8,
        domain,
    })
}
