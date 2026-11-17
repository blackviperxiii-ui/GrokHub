//! What on-device AI holds in memory now, and room for what loads next.
//!
//! The chat server (with its draft, in the same process) is read from the
//! sidecar slot each time, so every way it unloads stays right without a
//! copy to keep in step. Pictures, the picture guard and speech are short
//! jobs: each holds a [`Hold`] while it runs, and dropping it unregisters
//! the job on every path, errors and stops included.
//!
//! Before a job starts, [`reserve`] checks its need against the sampler's
//! free RAM and VRAM. If it doesn't fit, the least recently used entry that
//! may go is unloaded first. If it still doesn't fit, the job is declined
//! instead of overloading the computer.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::local_autoconfig::LaunchArgs;
use super::local_scale::{Room, RAM_SHARE_PCT};
use super::local_sidecar as sidecar;

/// What holds the memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The chat server, with its draft when one is paired.
    Chat,
    /// `sd-cli` drawing one picture.
    Picture,
    /// The picture guard checking one finished picture.
    PictureGuard,
    /// The on-device voice speaking one piece.
    Speech,
}

/// Memory in MB, split by where it sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mem {
    pub ram_mb: u64,
    pub vram_mb: u64,
}

impl Mem {
    pub const fn ram(ram_mb: u64) -> Self {
        Self { ram_mb, vram_mb: 0 }
    }

    fn total(self) -> u64 {
        self.ram_mb + self.vram_mb
    }

    fn plus(self, other: Self) -> Self {
        Self {
            ram_mb: self.ram_mb + other.ram_mb,
            vram_mb: self.vram_mb + other.vram_mb,
        }
    }

    fn fits_in(self, free: Self) -> bool {
        self.ram_mb <= free.ram_mb && self.vram_mb <= free.vram_mb
    }

    /// Shared memory: the GPU's share is RAM too.
    fn folded(self) -> Self {
        Self::ram(self.total())
    }
}

/// One thing in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub mem: Mem,
    /// Unix seconds it was last asked, answered or needed.
    pub last_used_secs: u64,
    /// It may not be unloaded now: a request is in flight, Keep loaded holds
    /// it, or it is a running job.
    pub pinned: bool,
}

/// What a new need gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admit {
    /// It fits beside what is loaded.
    Fits,
    /// It fits once these unload, least recently used first.
    Evict(Vec<Kind>),
    /// It doesn't fit even then; `need_mb` is what it needs free.
    Decline { need_mb: u64 },
}

/// A job that started this long after a reading may not be in it yet: its
/// process can still be loading weights when the sampler reads.
pub const SETTLE_MS: u64 = 10_000;

/// A running job's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldRec {
    id: u64,
    pub kind: Kind,
    pub mem: Mem,
    pub since_ms: u64,
}

/// What a need may use now: 80% of free RAM and all free VRAM in the reading
/// taken at `room_at_ms`, less the jobs that started too late to be in it.
pub fn budget(room: &Room, room_at_ms: u64, holds: &[HoldRec]) -> Mem {
    let mut free = Mem {
        ram_mb: room.free_ram_mb * RAM_SHARE_PCT / 100,
        vram_mb: room.free_vram_mb.unwrap_or(0),
    };
    for h in holds.iter().filter(|h| h.since_ms + SETTLE_MS > room_at_ms) {
        free.ram_mb = free.ram_mb.saturating_sub(h.mem.ram_mb);
        free.vram_mb = free.vram_mb.saturating_sub(h.mem.vram_mb);
    }
    free
}

/// Room for `need` with `free` left: as is, once the least recently used
/// entries that may go unload, or not at all.
pub fn admit(need: Mem, free: Mem, entries: &[Entry], unified: bool) -> Admit {
    let fold = |m: Mem| if unified { m.folded() } else { m };
    let need = fold(need);
    let mut free = fold(free);
    if need.fits_in(free) {
        return Admit::Fits;
    }
    let mut may_go: Vec<&Entry> = entries.iter().filter(|e| !e.pinned).collect();
    may_go.sort_by_key(|e| e.last_used_secs);
    let mut gone = Vec::new();
    for e in may_go {
        free = free.plus(fold(e.mem));
        gone.push(e.kind);
        if need.fits_in(free) {
            return Admit::Evict(gone);
        }
    }
    Admit::Decline {
        need_mb: need.total(),
    }
}

/// The part of a loaded chat model's `mb` that sits in RAM: all of it on the
/// CPU or with shared memory, the layers left off the GPU otherwise, and
/// none when the launch isn't known and used the GPU.
pub fn ram_share(mb: u64, launch: Option<&LaunchArgs>, bare_cpu: bool, unified: bool) -> u64 {
    if unified {
        return mb;
    }
    match launch {
        Some(a) if a.cpu_only || a.ngl == 0 => mb,
        Some(a) => {
            let all = u64::from(a.all_layers.max(1));
            mb * all.saturating_sub(u64::from(a.ngl)) / all
        }
        None if bare_cpu => mb,
        None => 0,
    }
}

/// The chat server's memory: the model split by its layers on the GPU, and
/// its draft (`draft_mb`) where the draft runs.
pub fn chat_mem(
    main_mb: u64,
    draft_mb: u64,
    launch: Option<&LaunchArgs>,
    bare_cpu: bool,
    unified: bool,
) -> Mem {
    let main_ram = ram_share(main_mb, launch, bare_cpu, unified);
    let draft_on_gpu = !unified
        && launch
            .and_then(|l| l.draft.as_ref())
            .is_some_and(|d| d.on_gpu);
    let draft = if draft_on_gpu {
        Mem {
            ram_mb: 0,
            vram_mb: draft_mb,
        }
    } else {
        Mem::ram(draft_mb)
    };
    Mem {
        ram_mb: main_ram,
        vram_mb: main_mb - main_ram,
    }
    .plus(draft)
}

/// Megabytes in a file on disk, or 0.
fn file_mb(p: &Path) -> u64 {
    std::fs::metadata(p).map_or(0, |m| m.len() / (1024 * 1024))
}

/// The loaded chat server as an entry.
fn chat_entry(r: &sidecar::ChatResident, unified: bool) -> Entry {
    let main_mb =
        super::local_swap::catalog_for(&r.model).map_or_else(|| file_mb(&r.model), |m| m.run_mb);
    let draft_mb = r
        .launch
        .as_ref()
        .and_then(|l| l.draft.as_ref())
        .and_then(|d| d.model.as_deref())
        .map_or(0, file_mb);
    Entry {
        kind: Kind::Chat,
        mem: chat_mem(main_mb, draft_mb, r.launch.as_ref(), r.bare_cpu, unified),
        last_used_secs: r.last_used_secs,
        pinned: r.pinned,
    }
}

/// Admissions run one at a time, so two jobs can't both count the same room.
static ADMIT: Mutex<()> = Mutex::new(());
/// Running jobs. A leaf lock: nothing else is called while it is held.
static HOLDS: Mutex<Vec<HoldRec>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A running job's place in the ledger, removed when dropped.
#[derive(Debug)]
pub struct Hold {
    id: u64,
}

impl Drop for Hold {
    fn drop(&mut self) {
        holds().retain(|h| h.id != self.id);
    }
}

fn holds() -> std::sync::MutexGuard<'static, Vec<HoldRec>> {
    HOLDS.lock().unwrap_or_else(|e| e.into_inner())
}

fn hold(kind: Kind, mem: Mem, now_ms: u64) -> Hold {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    holds().push(HoldRec {
        id,
        kind,
        mem,
        since_ms: now_ms,
    });
    Hold { id }
}

/// A job is running. Keep-warm and prewarm leave the chat model unloaded
/// meanwhile, so it doesn't take back the room the job needs; a chat
/// request still loads it.
pub fn held() -> bool {
    !holds().is_empty()
}

/// Room for a `kind` job that needs `need`, held until the [`Hold`] drops.
/// With `may_unload`, the chat server unloads first when that makes room
/// (it loads again on the next message). `Err` is the MB it needed free.
/// With no reading it starts, as it always has.
pub fn reserve(kind: Kind, need: Mem, may_unload: bool) -> Result<Hold, u64> {
    let _one = ADMIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = grokhub_core::now_ms();
    let Some((room, at)) = super::local_scale::room_at(now) else {
        return Ok(hold(kind, need, now));
    };
    let free = budget(&room, at, &holds());
    let resident = sidecar::resident();
    // A model still starting is in no reading yet and can't go: count only
    // what is free now.
    let chat = match (&resident.chat, resident.starting) {
        (Some(r), false) if may_unload => vec![chat_entry(r, room.unified)],
        _ => Vec::new(),
    };
    match admit(need, free, &chat, room.unified) {
        Admit::Fits => Ok(hold(kind, need, now)),
        // Only the chat server can be on the list. It unloads only if it is
        // still that model and still free to go.
        Admit::Evict(_) => match &resident.chat {
            Some(r) if sidecar::unload_for(reason(kind), &r.model) => Ok(hold(kind, need, now)),
            _ => Err(need.total()),
        },
        Admit::Decline { need_mb } => Err(need_mb),
    }
}

/// The health log's reason for unloading the chat model.
fn reason(kind: Kind) -> &'static str {
    match kind {
        Kind::Chat => "made room for another chat model",
        Kind::Picture => "made room for a picture",
        Kind::PictureGuard => "made room for the picture guard",
        Kind::Speech => "made room for the voice",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(free_vram_mb: Option<u64>, free_ram_mb: u64) -> Room {
        Room {
            free_vram_mb,
            free_ram_mb,
            cpu_busy_pct: None,
            on_battery: false,
            unified: false,
        }
    }

    fn chat(ram_mb: u64, vram_mb: u64, last_used_secs: u64, pinned: bool) -> Entry {
        Entry {
            kind: Kind::Chat,
            mem: Mem { ram_mb, vram_mb },
            last_used_secs,
            pinned,
        }
    }

    #[test]
    fn a_need_fits_unloads_the_least_recently_used_or_declines() {
        // 7 GB picture model plus 512 MB to spare: 7,680 MB of RAM.
        let picture = Mem::ram(7_680);
        // 80% of 10 GB free RAM is 8,192 MB: it fits beside the chat model.
        let free = budget(&room(None, 10_240), 0, &[]);
        assert_eq!(free, Mem::ram(8_192));
        assert_eq!(
            admit(picture, free, &[chat(5_000, 0, 9, false)], false),
            Admit::Fits
        );
        // 80% of 5 GB is 4,096 MB: only once 5 GB of chat model leaves RAM.
        let free = budget(&room(Some(2_048), 5_120), 0, &[]);
        assert_eq!(
            free,
            Mem {
                ram_mb: 4_096,
                vram_mb: 2_048
            }
        );
        assert_eq!(
            admit(picture, free, &[chat(5_000, 3_000, 9, false)], false),
            Admit::Evict(vec![Kind::Chat])
        );
        // Not even then; a pinned chat model can't go; nothing loaded.
        assert_eq!(
            admit(picture, free, &[chat(3_000, 0, 9, false)], false),
            Admit::Decline { need_mb: 7_680 }
        );
        assert_eq!(
            admit(picture, free, &[chat(5_000, 0, 9, true)], false),
            Admit::Decline { need_mb: 7_680 }
        );
        assert_eq!(
            admit(picture, free, &[], false),
            Admit::Decline { need_mb: 7_680 }
        );
        // Free VRAM doesn't hold a RAM need.
        let free = budget(&room(Some(24_576), 2_048), 0, &[]);
        assert_eq!(
            admit(picture, free, &[], false),
            Admit::Decline { need_mb: 7_680 }
        );
        // A VRAM need with no GPU reading declines.
        let gpu_need = Mem {
            ram_mb: 0,
            vram_mb: 1_000,
        };
        let free = budget(&room(None, 40_960), 0, &[]);
        assert_eq!(
            admit(gpu_need, free, &[], false),
            Admit::Decline { need_mb: 1_000 }
        );
    }

    #[test]
    fn the_oldest_unpinned_entries_go_first_and_only_as_many_as_needed() {
        let entries = [
            Entry {
                kind: Kind::Speech,
                ..chat(2_000, 0, 50, false)
            },
            chat(3_000, 0, 10, false),
            Entry {
                kind: Kind::Picture,
                ..chat(9_000, 0, 1, true)
            },
        ];
        // 1,000 free: the chat model (last used at 10) is enough.
        assert_eq!(
            admit(Mem::ram(3_500), Mem::ram(1_000), &entries, false),
            Admit::Evict(vec![Kind::Chat])
        );
        // Both unpinned ones, oldest first; the running picture never goes.
        assert_eq!(
            admit(Mem::ram(5_500), Mem::ram(1_000), &entries, false),
            Admit::Evict(vec![Kind::Chat, Kind::Speech])
        );
        assert_eq!(
            admit(Mem::ram(7_000), Mem::ram(1_000), &entries, false),
            Admit::Decline { need_mb: 7_000 }
        );
    }

    #[test]
    fn shared_memory_counts_the_gpu_share_as_ram() {
        let free = Mem {
            ram_mb: 4_000,
            vram_mb: 4_000,
        };
        // Split, 6 GB of RAM doesn't fit in 4 GB; shared, it fits in 8.
        assert_eq!(
            admit(Mem::ram(6_000), free, &[], false),
            Admit::Decline { need_mb: 6_000 }
        );
        assert_eq!(admit(Mem::ram(6_000), free, &[], true), Admit::Fits);
        assert_eq!(
            admit(Mem::ram(10_000), free, &[chat(0, 2_000, 1, false)], true),
            Admit::Evict(vec![Kind::Chat])
        );
    }

    #[test]
    fn a_job_newer_than_the_reading_is_taken_off_the_room() {
        let rec = |id, since_ms| HoldRec {
            id,
            kind: Kind::Speech,
            mem: Mem::ram(1_024),
            since_ms,
        };
        // The reading at 100 s: a job from 95 s may still be loading (in
        // 10 s of it), one from 80 s is in the reading already.
        let holds = [rec(1, 95_000), rec(2, 80_000)];
        assert_eq!(
            budget(&room(Some(500), 10_240), 100_000, &holds),
            Mem {
                ram_mb: 8_192 - 1_024,
                vram_mb: 500
            }
        );
        assert_eq!(
            budget(
                &room(None, 1_000),
                100_000,
                &[rec(3, 99_000), rec(4, 99_500)]
            ),
            Mem::ram(0)
        );
    }

    #[test]
    fn the_chat_server_splits_by_its_gpu_layers_and_counts_its_draft() {
        use super::super::local_autoconfig::{on_cpu, KvType, LaunchArgs};
        use super::super::local_spec::DraftArgs;
        let gpu = LaunchArgs {
            backend: grokhub_core::local_profile::Backend::Vulkan,
            ngl: 30,
            all_layers: 40,
            n_cpu_moe: None,
            ctx: 8192,
            kv: KvType::Q8,
            threads: 4,
            threads_batch: None,
            flash_attn: true,
            mlock: false,
            tensor_split: None,
            draft: None,
            cpu_only: false,
            run: Default::default(),
        };
        // 10 of 40 layers stay off the GPU: a quarter of 8 GB.
        assert_eq!(ram_share(8_000, Some(&gpu), false, false), 2_000);
        let all_gpu = LaunchArgs {
            ngl: 40,
            ..gpu.clone()
        };
        assert_eq!(ram_share(8_000, Some(&all_gpu), false, false), 0);
        assert_eq!(ram_share(8_000, Some(&on_cpu(&gpu)), false, false), 8_000);
        // Shared memory: the GPU's share is RAM too.
        assert_eq!(ram_share(8_000, Some(&all_gpu), false, true), 8_000);
        // One-shot defaults: on the CPU all of it, on the GPU none counted.
        assert_eq!(ram_share(8_000, None, true, false), 8_000);
        assert_eq!(ram_share(8_000, None, false, false), 0);

        assert_eq!(
            chat_mem(8_000, 0, Some(&gpu), false, false),
            Mem {
                ram_mb: 2_000,
                vram_mb: 6_000
            }
        );
        let with_draft = |on_gpu| LaunchArgs {
            draft: Some(Box::new(DraftArgs {
                kind: grokhub_core::local_draft::DraftKind::Simple,
                model: None,
                on_gpu,
                flags: Default::default(),
            })),
            ..gpu.clone()
        };
        // The draft is part of the same server: in VRAM beside the GPU
        // layers, or in RAM.
        assert_eq!(
            chat_mem(8_000, 500, Some(&with_draft(true)), false, false),
            Mem {
                ram_mb: 2_000,
                vram_mb: 6_500
            }
        );
        assert_eq!(
            chat_mem(8_000, 500, Some(&with_draft(false)), false, false),
            Mem {
                ram_mb: 2_500,
                vram_mb: 6_000
            }
        );
        assert_eq!(
            chat_mem(8_000, 500, Some(&with_draft(true)), false, true),
            Mem::ram(8_500)
        );
    }

    #[test]
    fn a_hold_leaves_the_ledger_when_it_drops() {
        let before = holds().len();
        let h = hold(Kind::Speech, Mem::ram(1_024), 5);
        assert!(held());
        assert_eq!(holds().iter().filter(|r| r.since_ms == 5).count(), 1);
        drop(h);
        assert_eq!(holds().iter().filter(|r| r.since_ms == 5).count(), 0);
        assert_eq!(holds().len(), before);
    }
}
