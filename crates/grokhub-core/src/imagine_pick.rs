//! Imagine picks its own model and quality from the prompt's words, the way
//! the router picks a chat model: nothing on screen names or picks it.
//!
//! - Words that need detail (photoreal, poster, 4k, words in the picture)
//!   get the full model at full quality.
//! - Words that ask for something rough (sketch, icon, meme, draft) get the
//!   cheaper, faster model.
//! - Detail wins when a prompt has both, since a cheap model garbles text.
//! - A prompt that names neither keeps the composer's Speed or Quality.
//!
//! A video ask (a video of, animate, timelapse) goes to the video model, its
//! length and resolution read from the same words.

use crate::imagine::{
    DEFAULT_IMAGINE_MODEL, DEFAULT_VIDEO_MODEL, FALLBACK_IMAGINE_MODEL, FALLBACK_VIDEO_MODEL,
};

/// What a prompt's words ask of the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagineTier {
    /// Something rough: the cheaper, faster model.
    Draft,
    /// Detail or words in the picture: the full model at full quality.
    Detail,
}

/// Words alone that ask for detail.
const DETAIL_WORDS: &[&str] = &[
    "photoreal",
    "photorealistic",
    "photorealism",
    "realistic",
    "hyperrealistic",
    "detailed",
    "intricate",
    "poster",
    "flyer",
    "banner",
    "billboard",
    "infographic",
    "typography",
    "lettering",
    "caption",
    "headline",
    "text",
    "logo",
    "4k",
    "8k",
    "uhd",
    "printable",
    "wallpaper",
];

/// Phrases that ask for detail, as whole words in order.
const DETAIL_PHRASES: &[&str] = &[
    "high detail",
    "fine detail",
    "high resolution",
    "high res",
    "hi res",
    "full hd",
    "album cover",
    "book cover",
    "magazine cover",
    "that says",
    "that reads",
    "with the words",
    "print quality",
    "best quality",
];

/// Words alone that ask for something rough.
const DRAFT_WORDS: &[&str] = &[
    "sketch",
    "sketchy",
    "doodle",
    "scribble",
    "draft",
    "rough",
    "icon",
    "icons",
    "emoji",
    "meme",
    "thumbnail",
    "placeholder",
    "wireframe",
];

/// Phrases that ask for something rough.
const DRAFT_PHRASES: &[&str] = &[
    "stick figure",
    "low res",
    "low resolution",
    "quick one",
    "quick test",
    "quick preview",
    "just a quick",
];

/// After `video`, a word that makes it a thing in the picture, not a clip.
const NOT_A_CLIP: &[&str] = &[
    "game",
    "games",
    "gamer",
    "call",
    "card",
    "player",
    "console",
    "store",
    "thumbnail",
    "poster",
    "cover",
    "camera",
    "screen",
    "tape",
];

/// Lowercase words: letters and digits, with every other character a break.
fn words(prompt: &str) -> Vec<String> {
    prompt
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn has_phrase(words: &[String], phrase: &str) -> bool {
    let want: Vec<&str> = phrase.split(' ').collect();
    words
        .windows(want.len())
        .any(|w| w.iter().zip(&want).all(|(a, b)| a == b))
}

/// A cue word or phrase, unless `no` or `without` comes right before a word
/// (`no text` asks for no words in the picture).
fn any_cue(words: &[String], singles: &[&str], phrases: &[&str]) -> bool {
    let negated = |i: usize| {
        i.checked_sub(1)
            .is_some_and(|p| matches!(words[p].as_str(), "no" | "without"))
    };
    words
        .iter()
        .enumerate()
        .any(|(i, w)| singles.contains(&w.as_str()) && !negated(i))
        || phrases.iter().any(|p| has_phrase(words, p))
}

/// Quoted words are words to draw in the picture: `a sign that reads "Open"`.
fn quotes_text(prompt: &str) -> bool {
    let straight = prompt.matches('"').count() >= 2;
    let curly = prompt.contains('\u{201c}') && prompt.contains('\u{201d}');
    straight || curly
}

/// The tier a prompt's words ask for; `None` when they name neither.
pub fn imagine_tier(prompt: &str) -> Option<ImagineTier> {
    let w = words(prompt);
    if quotes_text(prompt) || any_cue(&w, DETAIL_WORDS, DETAIL_PHRASES) {
        Some(ImagineTier::Detail)
    } else if any_cue(&w, DRAFT_WORDS, DRAFT_PHRASES) {
        Some(ImagineTier::Draft)
    } else {
        None
    }
}

/// The prompt asks for moving pictures: a video, an animation, a timelapse,
/// footage, a gif or slow motion. `video game` and the like don't count.
pub fn imagine_asks_video(prompt: &str) -> bool {
    let w = words(prompt);
    let video = w.iter().enumerate().any(|(i, x)| {
        matches!(x.as_str(), "video" | "videos")
            && w.get(i + 1)
                .is_none_or(|next| !NOT_A_CLIP.contains(&next.as_str()))
    });
    video
        || w.iter().any(|x| {
            matches!(
                x.as_str(),
                "animate" | "animates" | "timelapse" | "footage" | "gif" | "slowmo"
            )
        })
        || [
            "time lapse",
            "slow motion",
            "slow mo",
            "clip of",
            "short clip",
        ]
        .iter()
        .any(|p| has_phrase(&w, p))
}

/// A number written as digits or as a word up to twenty.
fn number(word: &str) -> Option<u32> {
    const NAMES: &[&str] = &[
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    word.parse()
        .ok()
        .or_else(|| NAMES.iter().position(|n| *n == word).map(|n| n as u32))
}

/// The clip lengths Imagine makes, in seconds.
const CLIP_SECS: &[u32] = &[6, 10, 15];

/// The shortest clip at least `n` seconds long, else the longest.
fn clip_at_least(n: u32) -> u32 {
    CLIP_SECS.iter().copied().find(|&s| s >= n).unwrap_or(15)
}

/// The clip length the words ask for: `10s`, `8 seconds`, `ten-second`
/// (rounded up to a length Imagine makes), or `short` and `long`.
pub fn imagine_video_secs(prompt: &str) -> Option<u32> {
    let w = words(prompt);
    for (i, x) in w.iter().enumerate() {
        // `10s`, `12sec`; past 30 it reads as a decade (`80s`), not a length.
        let glued = x
            .strip_suffix("sec")
            .or_else(|| x.strip_suffix("s"))
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|&n| n <= 30);
        let spaced = w.get(i + 1).and_then(|unit| {
            matches!(unit.as_str(), "s" | "sec" | "secs" | "second" | "seconds")
                .then(|| number(x))
                .flatten()
        });
        if let Some(n) = glued.or(spaced).filter(|&n| n > 0) {
            return Some(clip_at_least(n));
        }
    }
    if w.iter()
        .any(|x| matches!(x.as_str(), "long" | "longer" | "extended"))
    {
        Some(15)
    } else if w.iter().any(|x| matches!(x.as_str(), "short" | "brief")) {
        Some(6)
    } else {
        None
    }
}

/// The still Imagine makes: its model and whether it runs at full quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImagineStill {
    pub model: &'static str,
    pub quality: bool,
}

/// The model and quality for a still. `quality` is the composer's Speed or
/// Quality, kept when the words name neither.
pub fn imagine_still_for(prompt: &str, quality: bool) -> ImagineStill {
    match imagine_tier(prompt) {
        Some(ImagineTier::Detail) => ImagineStill {
            model: DEFAULT_IMAGINE_MODEL,
            quality: true,
        },
        Some(ImagineTier::Draft) => ImagineStill {
            model: FALLBACK_IMAGINE_MODEL,
            quality: false,
        },
        None => ImagineStill {
            model: DEFAULT_IMAGINE_MODEL,
            quality,
        },
    }
}

/// The clip Imagine makes. `None` keeps the composer's own length or resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImagineClip {
    pub model: &'static str,
    pub secs: Option<u32>,
    pub resolution: Option<&'static str>,
}

/// The model, length and resolution for a clip. A named resolution
/// (`480p`, `720p`, `1080p`) wins over the tier's; 1080p goes out only
/// where the API takes it (`video_resolution_for`).
pub fn imagine_clip_for(prompt: &str) -> ImagineClip {
    let w = words(prompt);
    let named = ["1080p", "720p", "480p"]
        .into_iter()
        .find(|r| w.iter().any(|x| x == r));
    let secs = imagine_video_secs(prompt);
    match imagine_tier(prompt) {
        Some(ImagineTier::Detail) => ImagineClip {
            model: DEFAULT_VIDEO_MODEL,
            secs,
            resolution: named.or(Some("1080p")),
        },
        Some(ImagineTier::Draft) => ImagineClip {
            model: FALLBACK_VIDEO_MODEL,
            secs: secs.or(Some(6)),
            resolution: named.or(Some("480p")),
        },
        None => ImagineClip {
            model: DEFAULT_VIDEO_MODEL,
            secs,
            resolution: named,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_words_and_quoted_text_get_the_full_model_at_full_quality() {
        for p in [
            "photorealistic portrait of an old fisherman",
            "a concert poster for a jazz night",
            "mountain lake at dawn, 4k",
            "highly detailed clockwork owl",
            "a bakery sign that reads \"Fresh Bread\"",
            "a mug with \u{201c}Hello\u{201d} on it",
            "logo for my-app with the words Night Shift",
        ] {
            assert_eq!(imagine_tier(p), Some(ImagineTier::Detail), "{p}");
            assert_eq!(
                imagine_still_for(p, false),
                ImagineStill {
                    model: "grok-imagine-image-2.0",
                    quality: true
                },
                "{p}"
            );
        }
    }

    #[test]
    fn rough_asks_get_the_cheaper_faster_model() {
        for p in [
            "quick sketch of a cat",
            "an app icon, flat",
            "a meme of a surprised dog",
            "rough draft of a castle layout",
            "stick figure doing yoga",
        ] {
            assert_eq!(imagine_tier(p), Some(ImagineTier::Draft), "{p}");
            assert_eq!(
                imagine_still_for(p, true),
                ImagineStill {
                    model: "grok-imagine-image",
                    quality: false
                },
                "{p}"
            );
        }
    }

    #[test]
    fn detail_wins_over_draft_and_no_cue_keeps_the_composer_choice() {
        // A cheap model garbles words, so text in the picture wins.
        assert_eq!(
            imagine_tier("a rough sketch of a poster with the text SALE"),
            Some(ImagineTier::Detail)
        );
        assert_eq!(imagine_tier("a red fox in the snow"), None);
        // Asking for no words in the picture is no text cue.
        assert_eq!(imagine_tier("a misty forest, no text"), None);
        // Words inside other words are no cue: "context", "sketchbook".
        assert_eq!(imagine_tier("a fox in context with a sketchbook"), None);
        assert_eq!(
            imagine_still_for("a red fox in the snow", false),
            ImagineStill {
                model: "grok-imagine-image-2.0",
                quality: false
            }
        );
        assert_eq!(
            imagine_still_for("a red fox in the snow", true),
            ImagineStill {
                model: "grok-imagine-image-2.0",
                quality: true
            }
        );
    }

    #[test]
    fn a_video_ask_is_moving_pictures_not_a_video_game() {
        for p in [
            "a video of waves at sunset",
            "animate this dragon breathing fire",
            "timelapse of a city at night",
            "time-lapse of clouds",
            "slow motion water drop",
            "a gif of a dancing cat",
            "short clip of rain on glass",
        ] {
            assert!(imagine_asks_video(p), "{p}");
        }
        for p in [
            "box art for a video game",
            "a video game character, pixel art",
            "an old video tape on a desk",
            "a paper clip on a desk",
            "an animated-looking fox portrait",
        ] {
            assert!(!imagine_asks_video(p), "{p}");
        }
    }

    #[test]
    fn the_clip_length_comes_from_the_words_rounded_up_to_what_imagine_makes() {
        assert_eq!(imagine_video_secs("a 10s clip of rain"), Some(10));
        assert_eq!(imagine_video_secs("8 seconds of waves"), Some(10));
        assert_eq!(imagine_video_secs("a ten-second loop"), Some(10));
        assert_eq!(imagine_video_secs("a 3 sec spin"), Some(6));
        assert_eq!(imagine_video_secs("a 12sec pan"), Some(15));
        assert_eq!(imagine_video_secs("30 seconds of fireworks"), Some(15));
        assert_eq!(imagine_video_secs("a long pan over hills"), Some(15));
        assert_eq!(imagine_video_secs("a short loop"), Some(6));
        assert_eq!(imagine_video_secs("waves at sunset"), None);
        // A number that isn't a length is no length, nor is a decade.
        assert_eq!(imagine_video_secs("3 cats dancing"), None);
        assert_eq!(imagine_video_secs("an 80s synthwave drive"), None);
    }

    #[test]
    fn a_clip_takes_its_model_and_resolution_from_the_words() {
        assert_eq!(
            imagine_clip_for("cinematic 4k drone shot over a canyon, 15 seconds"),
            ImagineClip {
                model: "grok-imagine-video-1.5",
                secs: Some(15),
                resolution: Some("1080p"),
            }
        );
        assert_eq!(
            imagine_clip_for("quick test of a bouncing ball"),
            ImagineClip {
                model: "grok-imagine-video",
                secs: Some(6),
                resolution: Some("480p"),
            }
        );
        assert_eq!(
            imagine_clip_for("rough draft of a spinning logo, 10s"),
            // "logo" is a detail word: words in the picture win.
            ImagineClip {
                model: "grok-imagine-video-1.5",
                secs: Some(10),
                resolution: Some("1080p"),
            }
        );
        assert_eq!(
            imagine_clip_for("a cat chasing a laser, 720p"),
            ImagineClip {
                model: "grok-imagine-video-1.5",
                secs: None,
                resolution: Some("720p"),
            }
        );
        assert_eq!(
            imagine_clip_for("waves at sunset"),
            ImagineClip {
                model: "grok-imagine-video-1.5",
                secs: None,
                resolution: None,
            }
        );
    }
}
