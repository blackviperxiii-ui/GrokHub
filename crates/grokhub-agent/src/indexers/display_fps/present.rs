//! Windows frame rate from the graphics kernel's own event log (ETW), the
//! source PresentMon, CapFrameX and RTSS read. A real-time trace session
//! listens for two events only: `Microsoft-Windows-DxgKrnl` Present (every
//! present through the Windows display kernel, any graphics API) and
//! `Microsoft-Windows-DXGI` Present start. Each event says which process
//! presented; Loadout counts them per process and reports the busiest one
//! that isn't itself or the desktop compositor.
//!
//! Nothing here opens, reads or touches another process: the kernel writes
//! the events and the process list comes from a Toolhelp snapshot of names.
//! Starting a trace session needs an administrator or membership in the
//! built-in Performance Log Users group; see [`super::windows_setup`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS};
use windows_sys::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
    CONTROLTRACE_HANDLE, ENABLE_TRACE_PARAMETERS, ENABLE_TRACE_PARAMETERS_VERSION_2,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_FILTER_DESCRIPTOR, EVENT_FILTER_EVENT_ID,
    EVENT_FILTER_TYPE_EVENT_ID, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, PROCESSTRACE_HANDLE,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
};

use super::{busiest_presenter, FpsBlock};

/// Microsoft-Windows-DxgKrnl {802EC45A-1E99-4B83-9920-87C98277BA9D}.
const DXGKRNL: GUID = GUID::from_u128(0x802ec45a_1e99_4b83_9920_87c98277ba9d);
/// Microsoft-Windows-DXGI {CA11C036-0102-4A2D-A6AD-F03CFED5D3C9}.
const DXGI: GUID = GUID::from_u128(0xca11c036_0102_4a2d_a6ad_f03cfed5d3c9);
/// DxgKrnl Present_Info; keywords Base | Present.
const DXGKRNL_PRESENT: u16 = 0x00b8;
const DXGKRNL_KEYWORDS: u64 = 0x0800_0001;
/// DXGI Present_Start; keyword Events.
const DXGI_PRESENT: u16 = 0x002a;
const DXGI_KEYWORDS: u64 = 0x2;
const TRACE_LEVEL_INFORMATIONAL: u8 = 4;

const SESSION: &str = "Loadout Frame Rate";

/// Presents per process since the last read, written by the trace callback.
/// One session at a time, so one table.
static COUNTS: Mutex<Option<HashMap<u32, [u64; 2]>>> = Mutex::new(None);

pub(crate) struct Session {
    control: CONTROLTRACE_HANDLE,
    reader: Option<std::thread::JoinHandle<()>>,
    since: Instant,
    names: HashMap<u32, String>,
    pub(crate) presenter: Option<String>,
}

impl Session {
    pub(crate) fn start() -> Result<Self, FpsBlock> {
        stop_named();
        let mut props = Properties::new();
        let name = wide(SESSION);
        let mut control = CONTROLTRACE_HANDLE { Value: 0 };
        // SAFETY: props is a correctly sized EVENT_TRACE_PROPERTIES with room
        // for the session name after it; name is NUL-terminated.
        let rc = unsafe { StartTraceW(&mut control, name.as_ptr(), props.as_mut_ptr()) };
        match rc {
            ERROR_SUCCESS => {}
            ERROR_ACCESS_DENIED => return Err(FpsBlock::NeedsPermission),
            _ => return Err(FpsBlock::NoTracepoint),
        }
        let session = Session {
            control,
            reader: None,
            since: Instant::now(),
            names: HashMap::new(),
            presenter: None,
        };
        let enabled = enable(control, &DXGKRNL, DXGKRNL_PRESENT, DXGKRNL_KEYWORDS);
        // DXGI is a second opinion; DxgKrnl alone still counts every API.
        let _ = enable(control, &DXGI, DXGI_PRESENT, DXGI_KEYWORDS);
        if enabled != ERROR_SUCCESS {
            return Err(if enabled == ERROR_ACCESS_DENIED {
                FpsBlock::NeedsPermission
            } else {
                FpsBlock::NoTracepoint
            });
        }
        if let Ok(mut c) = COUNTS.lock() {
            *c = Some(HashMap::new());
        }
        let mut session = session;
        session.reader = Some(open_reader()?);
        Ok(session)
    }

    /// Frames per second of the busiest presenter since the last call, and
    /// remembers its name in `presenter`. `None` when the reader stopped.
    pub(crate) fn rate(&mut self) -> Option<f32> {
        if self.reader.as_ref().is_none_or(|r| r.is_finished()) {
            return None;
        }
        let now = Instant::now();
        let secs = now.duration_since(self.since).as_secs_f64();
        self.since = now;
        let round = COUNTS.lock().ok()?.as_mut().map(std::mem::take)?;
        if secs <= 0.0 {
            return None;
        }
        let names = &mut self.names;
        let mut lookup = |pid: u32| {
            if !names.contains_key(&pid) {
                if names.len() > 512 {
                    names.clear();
                }
                names.extend(process_names());
            }
            names.get(&pid).cloned()
        };
        match busiest_presenter(&round, std::process::id(), &mut lookup) {
            Some((pid, n)) => {
                self.presenter = lookup(pid);
                Some((n as f64 / secs) as f32)
            }
            None => {
                self.presenter = None;
                Some(0.0)
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let mut props = Properties::new();
        // SAFETY: stopping our own session by handle; props has room for the
        // name the call writes back.
        unsafe {
            ControlTraceW(
                self.control,
                std::ptr::null(),
                props.as_mut_ptr(),
                EVENT_TRACE_CONTROL_STOP,
            )
        };
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        if let Ok(mut c) = COUNTS.lock() {
            *c = None;
        }
    }
}

/// A session left behind by a crash keeps its name; stop it so the start
/// below doesn't fail with "already exists".
fn stop_named() {
    let mut props = Properties::new();
    let name = wide(SESSION);
    // SAFETY: as in Session::drop, by name instead of handle.
    unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE { Value: 0 },
            name.as_ptr(),
            props.as_mut_ptr(),
            EVENT_TRACE_CONTROL_STOP,
        )
    };
}

/// Turn a provider on for just the one event; if this Windows can't filter
/// by event id, turn it on unfiltered (the callback drops the rest).
fn enable(control: CONTROLTRACE_HANDLE, provider: &GUID, id: u16, keywords: u64) -> u32 {
    let rc = enable_with(control, provider, Some(id), keywords);
    if rc == ERROR_SUCCESS || rc == ERROR_ACCESS_DENIED {
        return rc;
    }
    enable_with(control, provider, None, keywords)
}

fn enable_with(
    control: CONTROLTRACE_HANDLE,
    provider: &GUID,
    id: Option<u16>,
    keywords: u64,
) -> u32 {
    let id = match id {
        Some(id) => id,
        // SAFETY: no filter, no parameters.
        None => unsafe {
            return EnableTraceEx2(
                control,
                provider,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                TRACE_LEVEL_INFORMATIONAL,
                keywords,
                0,
                0,
                std::ptr::null(),
            );
        },
    };
    let mut ids = EVENT_FILTER_EVENT_ID {
        FilterIn: true,
        Reserved: 0,
        Count: 1,
        Events: [id],
    };
    let mut filter = EVENT_FILTER_DESCRIPTOR {
        Ptr: &mut ids as *mut EVENT_FILTER_EVENT_ID as u64,
        Size: std::mem::size_of::<EVENT_FILTER_EVENT_ID>() as u32,
        Type: EVENT_FILTER_TYPE_EVENT_ID,
    };
    let params = ENABLE_TRACE_PARAMETERS {
        Version: ENABLE_TRACE_PARAMETERS_VERSION_2,
        EnableProperty: 0,
        ControlFlags: 0,
        SourceId: GUID::from_u128(0),
        EnableFilterDesc: &mut filter,
        FilterDescCount: 1,
    };
    // SAFETY: params, filter and ids outlive the call.
    unsafe {
        EnableTraceEx2(
            control,
            provider,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER,
            TRACE_LEVEL_INFORMATIONAL,
            keywords,
            0,
            0,
            &params,
        )
    }
}

/// Open the session for reading and pump it on a thread until it stops.
fn open_reader() -> Result<std::thread::JoinHandle<()>, FpsBlock> {
    let mut name = wide(SESSION);
    // SAFETY: zeroed is a valid EVENT_TRACE_LOGFILEW (plain data and
    // nullable pointers); the fields OpenTraceW needs are set below.
    let mut log: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
    log.LoggerName = name.as_mut_ptr();
    log.Anonymous1.ProcessTraceMode =
        PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    log.Anonymous2.EventRecordCallback = Some(on_event);
    // SAFETY: log is filled in for a real-time, event-record consumer.
    let handle = unsafe { OpenTraceW(&mut log) };
    if handle.Value == u64::MAX || handle.Value == u32::MAX as u64 {
        return Err(FpsBlock::NoTracepoint);
    }
    std::thread::Builder::new()
        .name("home-fps-etw".into())
        .spawn(move || {
            let handle = PROCESSTRACE_HANDLE {
                Value: handle.Value,
            };
            // SAFETY: blocks until the session stops; then the handle is ours
            // to close.
            unsafe {
                ProcessTrace(&handle, 1, std::ptr::null(), std::ptr::null());
                CloseTrace(handle);
            }
        })
        .map_err(|_| FpsBlock::NoTracepoint)
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    // SAFETY: ETW passes a valid record for the duration of the call.
    let Some(header) = (unsafe { record.as_ref() }).map(|r| &r.EventHeader) else {
        return;
    };
    let id = header.EventDescriptor.Id;
    let column = if same(&header.ProviderId, &DXGKRNL) && id == DXGKRNL_PRESENT {
        0
    } else if same(&header.ProviderId, &DXGI) && id == DXGI_PRESENT {
        1
    } else {
        return;
    };
    if let Ok(mut guard) = COUNTS.lock() {
        if let Some(map) = guard.as_mut() {
            map.entry(header.ProcessId).or_default()[column] += 1;
        }
    }
}

fn same(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// Every running process's exe name by pid, from a Toolhelp snapshot (names
/// only; no handle to any process).
fn process_names() -> Vec<(u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    // SAFETY: plain Toolhelp calls; the entry's dwSize is set before use and
    // the snapshot handle is closed at the end.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            out.push((
                entry.th32ProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..len]),
            ));
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    out
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// EVENT_TRACE_PROPERTIES followed by room for the session name, as
/// StartTraceW and ControlTraceW want it.
struct Properties {
    buf: Vec<u64>,
}

impl Properties {
    fn new() -> Self {
        let head = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
        let total = head + 1024 * 2;
        let mut buf = vec![0u64; total.div_ceil(8)];
        // SAFETY: buf is zeroed, 8-byte aligned and at least head bytes long.
        let props = unsafe { &mut *(buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) };
        props.Wnode.BufferSize = total as u32;
        props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        props.Wnode.ClientContext = 1; // QPC timestamps
        props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        props.LoggerNameOffset = head as u32;
        props.BufferSize = 64; // KB per buffer
        props.FlushTimer = 1; // second
        Properties { buf }
    }

    fn as_mut_ptr(&mut self) -> *mut EVENT_TRACE_PROPERTIES {
        self.buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES
    }
}
