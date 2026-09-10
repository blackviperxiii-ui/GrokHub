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
    waveInStop, waveInUnprepareHeader, waveOutClose, waveOutOpen, waveOutPrepareHeader,
    waveOutReset, waveOutUnprepareHeader, waveOutWrite, HWAVEIN, HWAVEOUT, PlaySoundW,
    WAVEFORMATEX, WAVEHDR, CALLBACK_NULL, SND_FILENAME, SND_NODEFAULT, SND_SYNC, WAVE_FORMAT_PCM,
    WAVE_MAPPER, WHDR_DONE,
};
use windows_sys::Win32::Media::Multimedia::mciSendStringW;

const RATE: u32 = 24_000;
const FRAME: usize = 4800; // 100 ms s16le mono
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
    bufs: [Vec<u8>; 2],
    hdrs: [WAVEHDR; 2],
    prepared: [bool; 2],
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
        let err = unsafe {
            waveInOpen(
                &mut handle,
                WAVE_MAPPER,
                &fmt,
                0,
                0,
                CALLBACK_NULL,
            )
        };
        if err != MMSYSERR_NOERROR || handle.is_null() {
            return None;
        }
        let mut mic = Self {
            handle,
            inner: Box::new(WaveInInner {
                bufs: [vec![0u8; FRAME], vec![0u8; FRAME]],
                hdrs: [empty_hdr(), empty_hdr()],
                prepared: [false, false],
                next: 0,
            }),
        };
        for i in 0..2 {
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
                self.inner.next = 1 - i;
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
            for i in 0..2 {
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

struct OutSlot {
    _buf: Vec<u8>,
    hdr: Box<WAVEHDR>,
}

pub struct WaveOut {
    handle: HWAVEOUT,
    pending: Vec<OutSlot>,
}

impl WaveOut {
    pub fn start() -> Option<Self> {
        let fmt = pcm_fmt();
        let mut handle: HWAVEOUT = ptr::null_mut();
        let err = unsafe {
            waveOutOpen(
                &mut handle,
                WAVE_MAPPER,
                &fmt,
                0,
                0,
                CALLBACK_NULL,
            )
        };
        if err != MMSYSERR_NOERROR || handle.is_null() {
            return None;
        }
        Some(Self {
            handle,
            pending: Vec::new(),
        })
    }

    pub fn push(&mut self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        self.reap();
        let mut buf = pcm.to_vec();
        let mut hdr = Box::new(WAVEHDR {
            lpData: buf.as_mut_ptr(),
            dwBufferLength: buf.len() as u32,
            dwBytesRecorded: 0,
            dwUser: 0,
            dwFlags: 0,
            dwLoops: 0,
            lpNext: ptr::null_mut(),
            reserved: 0,
        });
        unsafe {
            if waveOutPrepareHeader(self.handle, hdr.as_mut(), std::mem::size_of::<WAVEHDR>() as u32)
                != MMSYSERR_NOERROR
            {
                return;
            }
            if waveOutWrite(self.handle, hdr.as_mut(), std::mem::size_of::<WAVEHDR>() as u32)
                != MMSYSERR_NOERROR
            {
                let _ = waveOutUnprepareHeader(
                    self.handle,
                    hdr.as_mut(),
                    std::mem::size_of::<WAVEHDR>() as u32,
                );
                return;
            }
        }
        self.pending.push(OutSlot { _buf: buf, hdr });
    }

    fn reap(&mut self) {
        let mut keep = Vec::new();
        for mut slot in self.pending.drain(..) {
            if (slot.hdr.dwFlags & WHDR_DONE) != 0 {
                unsafe {
                    let _ = waveOutUnprepareHeader(
                        self.handle,
                        slot.hdr.as_mut(),
                        std::mem::size_of::<WAVEHDR>() as u32,
                    );
                }
            } else {
                keep.push(slot);
            }
        }
        self.pending = keep;
    }
}

impl Drop for WaveOut {
    fn drop(&mut self) {
        unsafe {
            let _ = waveOutReset(self.handle);
            for mut slot in self.pending.drain(..) {
                let _ = waveOutUnprepareHeader(
                    self.handle,
                    slot.hdr.as_mut(),
                    std::mem::size_of::<WAVEHDR>() as u32,
                );
            }
            let _ = waveOutClose(self.handle);
        }
    }
}

/// Capture `secs` of 24 kHz s16le mono. Used for PTT and the duplex fallback.
pub fn record_pcm_secs(secs: u32) -> Result<Vec<u8>, String> {
    let mut mic = WaveIn::start().ok_or_else(|| {
        "Windows microphone unavailable — check Settings → Privacy → Microphone for GrokHub"
            .to_string()
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

pub fn write_wav_s16le_24k(path: &Path, pcm: &[u8]) -> Result<(), String> {
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

pub fn record_wav(path: &Path, secs: u32) -> Result<(), String> {
    let pcm = record_pcm_secs(secs)?;
    write_wav_s16le_24k(path, &pcm)
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
        if mciSendStringW(open.as_ptr(), err.as_mut_ptr(), err.len() as u32, ptr::null_mut()) != 0 {
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
    fn wav_header_is_s16le_24k_mono() {
        let dir = std::env::temp_dir().join(format!("grokhub-wav-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.wav");
        write_wav_s16le_24k(&path, b"PCM!").unwrap();
        let b = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(b.starts_with(b"RIFF"));
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(&b[22..24], &1u16.to_le_bytes());
        assert_eq!(&b[24..28], &24000u32.to_le_bytes());
        assert_eq!(&b[44..], b"PCM!");
    }

    #[test]
    fn wave_headers_live_on_the_heap() {
        let src = include_str!("win_audio.rs");
        assert!(
            src.contains("inner: Box<WaveInInner>") && src.contains("hdr: Box<WAVEHDR>"),
            "prepared WAVEHDRs must not move when WaveIn/WaveOut is moved: {src}"
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
