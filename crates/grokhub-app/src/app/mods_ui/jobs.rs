//! Mod lookups run on their own threads through one [`ModClient`], so the
//! page never waits on a source. Each lookup has a slot: what the cache had
//! at once, then the source's answer when it comes. A slot is asked once per
//! session; Retry forgets it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui;
use grokhub_core::mods::{Keys, ModClient, ModDetail, ModFile, Page, Query, Source, SourceError};

/// One lookup: the last value, whether it is still being asked, and the
/// line to show when it failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Slot<T> {
    pub value: Option<T>,
    pub busy: bool,
    pub error: Option<String>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: None,
            busy: false,
            error: None,
        }
    }
}

#[derive(Default)]
struct Jobs {
    client: Option<Arc<ModClient>>,
    pages: HashMap<String, Slot<Page>>,
    details: HashMap<String, Slot<ModDetail>>,
    files: HashMap<String, Slot<Vec<ModFile>>>,
    /// The last word on saving or forgetting a key.
    key_note: Option<String>,
}

fn jobs() -> std::sync::MutexGuard<'static, Jobs> {
    static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The CurseForge key Loadout ships with, when the build has one.
fn shipped_curseforge_key() -> Option<String> {
    option_env!("CURSEFORGE_API_KEY")
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_owned)
}

/// The keys the sources may use: the user's sealed Nexus and Steam keys and
/// the shipped CurseForge key.
fn read_keys() -> Keys {
    let dir = crate::config::config_dir();
    Keys {
        nexus: grokhub_agent::harness::open_mod_key(&dir, Source::Nexus.id()),
        steam: grokhub_agent::harness::open_mod_key(&dir, Source::Workshop.id()),
        curseforge: shipped_curseforge_key(),
    }
}

fn client(j: &mut Jobs) -> Arc<ModClient> {
    j.client
        .get_or_insert_with(|| {
            Arc::new(ModClient::new(
                Box::new(crate::mods_fetch::ModsNet),
                &crate::config::config_dir(),
                read_keys(),
            ))
        })
        .clone()
}

/// The one client the page and the chat's mod tools share.
pub(super) fn shared_client() -> Arc<ModClient> {
    client(&mut jobs())
}

/// Whether the user (or the build) has a key for this source.
pub(super) fn has_key(source: Source) -> bool {
    let c = client(&mut jobs());
    let keys = c.keys();
    match source {
        Source::Nexus => keys.nexus.is_some(),
        Source::Workshop => keys.steam.is_some(),
        Source::CurseForge => keys.curseforge.is_some(),
        Source::Thunderstore | Source::Modrinth => true,
    }
}

/// Start over with the keys as they are now; every lookup is asked again.
pub(super) fn reset() {
    let mut j = jobs();
    j.client = None;
    j.pages.clear();
    j.details.clear();
    j.files.clear();
}

pub(super) fn key_note() -> Option<String> {
    jobs().key_note.clone()
}

pub(super) fn set_key_note(note: String) {
    jobs().key_note = Some(note);
}

fn page_key(game_id: &str, source: Source, q: &Query) -> String {
    format!(
        "{game_id}|{}|{:?}|{}|{}",
        source.id(),
        q.browse,
        q.page,
        q.text().unwrap_or_default().to_lowercase()
    )
}

fn mod_key(game_id: &str, source: Source, id: &str) -> String {
    format!("{game_id}|{}|{id}", source.id())
}

/// Ask once: the cache's value now, the source's answer on a thread.
fn ask<T, C, A>(
    ctx: &egui::Context,
    map: fn(&mut Jobs) -> &mut HashMap<String, Slot<T>>,
    key: String,
    cached: C,
    fetch: A,
) -> Slot<T>
where
    T: Clone + Send + 'static,
    C: FnOnce(&ModClient) -> Option<T>,
    A: FnOnce(&ModClient) -> Result<T, SourceError> + Send + 'static,
{
    let mut j = jobs();
    if let Some(slot) = map(&mut j).get(&key) {
        return slot.clone();
    }
    let c = client(&mut j);
    let slot = Slot {
        value: cached(&c),
        busy: true,
        error: None,
    };
    map(&mut j).insert(key.clone(), slot.clone());
    drop(j);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let got = fetch(&c);
        let mut j = jobs();
        let slot = map(&mut j).entry(key).or_default();
        slot.busy = false;
        match got {
            Ok(v) => {
                slot.value = Some(v);
                slot.error = None;
            }
            Err(e) => slot.error = Some(e.line()),
        }
        drop(j);
        ctx.request_repaint();
    });
    slot
}

/// One page of mods: the cached page at once, the fresh one when it comes.
pub(super) fn page(
    ctx: &egui::Context,
    game_id: &'static str,
    source: Source,
    q: &Query,
) -> Slot<Page> {
    let q = q.clone();
    let q2 = q.clone();
    ask(
        ctx,
        |j| &mut j.pages,
        page_key(game_id, source, &q),
        move |c| c.cached_search(game_id, source, &q2),
        move |c| {
            c.search(game_id, source, &q, grokhub_core::now_ms())
                .map(|g| g.value)
        },
    )
}

/// A page from the cache only, never the network (Offline only).
pub(super) fn cached_page(game_id: &str, source: Source, q: &Query) -> Option<Page> {
    let c = client(&mut jobs());
    c.cached_search(game_id, source, q)
}

pub(super) fn detail(
    ctx: &egui::Context,
    game_id: &'static str,
    source: Source,
    id: &str,
) -> Slot<ModDetail> {
    let id = id.to_string();
    ask(
        ctx,
        |j| &mut j.details,
        mod_key(game_id, source, &id),
        |_| None,
        move |c| {
            c.detail(game_id, source, &id, grokhub_core::now_ms())
                .map(|g| g.value)
        },
    )
}

pub(super) fn files(
    ctx: &egui::Context,
    game_id: &'static str,
    source: Source,
    id: &str,
) -> Slot<Vec<ModFile>> {
    let id = id.to_string();
    ask(
        ctx,
        |j| &mut j.files,
        mod_key(game_id, source, &id),
        |_| None,
        move |c| {
            c.files(game_id, source, &id, grokhub_core::now_ms())
                .map(|g| g.value)
        },
    )
}

/// Forget one page's slot so the next frame asks again.
pub(super) fn retry_page(game_id: &str, source: Source, q: &Query) {
    jobs().pages.remove(&page_key(game_id, source, q));
}

/// Forget one mod's detail and files.
pub(super) fn retry_mod(game_id: &str, source: Source, id: &str) {
    let key = mod_key(game_id, source, id);
    let mut j = jobs();
    j.details.remove(&key);
    j.files.remove(&key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::mods::Browse;

    #[test]
    fn page_keys_tell_queries_apart_and_ignore_case() {
        let a = page_key("minecraft", Source::Modrinth, &Query::search("Sodium"));
        let b = page_key("minecraft", Source::Modrinth, &Query::search("sodium"));
        assert_eq!(a, b);
        let mut next = Query::search("sodium");
        next.page = 1;
        assert_ne!(a, page_key("minecraft", Source::Modrinth, &next));
        assert_ne!(
            page_key(
                "minecraft",
                Source::Modrinth,
                &Query::browse(Browse::Popular)
            ),
            page_key(
                "minecraft",
                Source::CurseForge,
                &Query::browse(Browse::Popular)
            )
        );
    }
}
