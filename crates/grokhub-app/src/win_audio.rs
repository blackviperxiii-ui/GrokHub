//! Windows capture/playback via waveIn/waveOut (24 kHz s16le mono).
//!
//! WAVEHDR pointers are handed to winmm. The header *address* must stay put
//! until Unprepare — moving a prepared header (return `Some(mic)`, push onto
//! a Vec) is STATUS_HEAP_CORRUPTION (0xc0000374).

#![cfg(windows)]

use std::path::Path;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Media::Audio::{
    waveInAddBuffer, waveInClose, waveInOpen, waveInPrepareHeader, waveInReset, waveInStart,
    waveInStop, waveInUnprepareHeader, PlaySoundW, CALLBACK_NULL, HWAVEIN, SND_FILENAME,
    SND_NODEFAULT, SND_SYNC, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
};
use windows_sys::Win32::Media::Multimedia::mciSendStringW;

pub(crate) const RATE: u32 = 24_000;
const FRAME: usize = 4800; // 100 ms s16le mono
/// Queued capture buffers: 800 ms of slack before a busy moment drops audio.
const BUFS: usize = 8;
const MMSYSERR_NOERROR: u32 = 0;

fn pcm_fmt() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    }
}

fn empty_hdr() -> WAVEHDR {
    WAVEHDR {
        lpData: ptr::null_mut(),
        dwBufferLength: 0,
        dwBytesRecorded: 0,
        dwUser: 0,
        dwFlags: 0,
        dwLoops: 0,
        lpNext: ptr::null_mut(),
        reserved: 0,
    }
}

struct WaveInInner {
    bufs: [Vec<u8>; BUFS],
    hdrs: [WAVEHDR; BUFS],
    prepared: [bool; BUFS],
    next: usize,
}

/// Heap-stable WAVEHDRs. Moving `WaveIn` only moves this box pointer.
pub struct WaveIn {
    handle: HWAVEIN,
    inner: Box<WaveInInner>,
}

impl WaveIn {
    pub fn start() -> Option<Self> {
        let fmt = pcm_fmt();
        let mut handle: HWAVEIN = ptr::null_mut();
        let err = unsafe { waveInOpen(&mut handle, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL) };
        if err != MMSYSERR_NOERROR || handle.is_null() {
            return None;
        }
        let mut mic = Self {
            handle,
            inner: Box::new(WaveInInner {
                bufs: std::array::from_fn(|_| vec![0u8; FRAME]),
                hdrs: std::array::from_fn(|_| empty_hdr()),
                prepared: [false; BUFS],
                next: 0,
            }),
        };
        for i in 0..BUFS {
            if !mic.queue(i) {
                return None;
            }
        }
        let err = unsafe { waveInStart(handle) };
        if err != MMSYSERR_NOERROR {
            return None;
        }
        Some(mic)
    }

    fn queue(&mut self, i: usize) -> bool {
        if self.inner.prepared[i] {
            unsafe {
                let _ = waveInUnprepareHeader(
                    self.handle,
                    &mut self.inner.hdrs[i],
                    std::mem::size_of::<WAVEHDR>() as u32,
                );
            }
            self.inner.prepared[i] = false;
        }
        self.inner.hdrs[i] = WAVEHDR {
            lpData: self.inner.bufs[i].as_mut_ptr(),
            dwBufferLength: self.inner.bufs[i].len() as u32,
            dwBytesRecorded: 0,
            dwUser: 0,
            dwFlags: 0,
            dwLoops: 0,
            lpNext: ptr::null_mut(),
            reserved: 0,
        };
        unsafe {
            if waveInPrepareHeader(
                self.handle,
                &mut self.inner.hdrs[i],
                std::mem::size_of::<WAVEHDR>() as u32,
            ) != MMSYSERR_NOERROR
            {
                return false;
            }
            self.inner.prepared[i] = true;
            waveInAddBuffer(
                self.handle,
                &mut self.inner.hdrs[i],
                std::mem::size_of::<WAVEHDR>() as u32,
            ) == MMSYSERR_NOERROR
        }
    }

    pub fn read_frame(&mut self) -> Option<Vec<u8>> {
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            let i = self.inner.next;
            let done = (self.inner.hdrs[i].dwFlags & WHDR_DONE) != 0;
            if done {
                let n = self.inner.hdrs[i].dwBytesRecorded as usize;
                let pcm = self.inner.bufs[i][..n.min(self.inner.bufs[i].len())].to_vec();
                if !self.queue(i) {
                    return None;
                }
                self.inner.next = (i + 1) % BUFS;
                return Some(pcm);
            }
            if Instant::now() >= deadline {
                // Timeout is idle, not a dead device — duplex Voice must keep the loop.
                return Some(Vec::new());
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }
}

impl Drop for WaveIn {
    fn drop(&mut self) {
        unsafe {
            let _ = waveInStop(self.handle);
            let _ = waveInReset(self.handle);
            for i in 0..BUFS {
                if self.inner.prepared[i] {
                    let _ = waveInUnprepareHeader(
                        self.handle,
                        &mut self.inner.hdrs[i],
                        std::mem::size_of::<WAVEHDR>() as u32,
                    );
                    self.inner.prepared[i] = false;
                }
            }
            let _ = waveInClose(self.handle);
        }
    }
}

/// Capture `secs` of 24 kHz s16le mono (the audio check's clip).
pub fn record_pcm_secs(secs: u32) -> Result<Vec<u8>, String> {
    let mut mic = WaveIn::start().ok_or_else(|| {
        format!(
            "Windows microphone unavailable — check Settings → Privacy → Microphone for {}",
            grokhub_core::brand::product_name()
        )
    })?;
    let want = (RATE as usize) * 2 * secs.max(1) as usize;
    let mut out = Vec::with_capacity(want);
    let deadline = Instant::now() + Duration::from_secs(secs as u64 + 2);
    while out.len() < want {
        if Instant::now() >= deadline {
            break;
        }
        match mic.read_frame() {
            Some(frame) if !frame.is_empty() => out.extend_from_slice(&frame),
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    if out.len() < 64 {
        return Err("empty recording".into());
    }
    Ok(out)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn play_file(path: &Path) -> Result<(), String> {
    let dest = path.to_str().ok_or("audio path")?;
    let bytes = std::fs::read(path).unwrap_or_default();
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        let w = wide(dest);
        let flags = SND_FILENAME | SND_NODEFAULT | SND_SYNC;
        let ok = unsafe { PlaySoundW(w.as_ptr(), ptr::null_mut(), flags) };
        if ok != 0 {
            return Ok(());
        }
    }
    let alias = format!("grokhubplay{}", std::process::id());
    let open = wide(&format!("open \"{dest}\" alias {alias}"));
    let play = wide(&format!("play {alias} wait"));
    let close = wide(&format!("close {alias}"));
    unsafe {
        let mut err = [0u16; 128];
        if mciSendStringW(
            open.as_ptr(),
            err.as_mut_ptr(),
            err.len() as u32,
            ptr::null_mut(),
        ) != 0
        {
            return Err("Windows media play failed (need a microphone-capable audio stack)".into());
        }
        let _ = mciSendStringW(play.as_ptr(), ptr::null_mut(), 0, ptr::null_mut());
        let _ = mciSendStringW(close.as_ptr(), ptr::null_mut(), 0, ptr::null_mut());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_headers_live_on_the_heap() {
        let src = include_str!("win_audio.rs");
        assert!(
            src.contains("inner: Box<WaveInInner>"),
            "prepared WAVEHDRs must not move when WaveIn is moved: {src}"
        );
        assert!(
            src.contains("STATUS_HEAP_CORRUPTION") || src.contains("0xc0000374"),
            "keep the winmm lifetime rule next to the pin: {src}"
        );
    }

    #[test]
    fn wave_in_start_does_not_heap_corrupt() {
        // Open + move + drop. A prepared stack WAVEHDR crashes here (0xc0000374).
        let mic = WaveIn::start();
        drop(mic);
    }
}
