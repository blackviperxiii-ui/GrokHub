//! A span file's newest lines, kept parsed in memory and followed as the file
//! grows: each read parses only the bytes appended since the last one. It
//! reads what [`read_spans_tail`](super::read_spans_tail) reads (the last
//! [`SPAN_TAIL_BYTES`] of the file, empty lines skipped, a cut first line
//! dropped), so callers see the same spans without re-parsing megabytes per
//! call. A file that shrank or was replaced is read again from scratch.

use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::span::{Span, SPAN_TAIL_BYTES};

/// Bytes of the file's start remembered to notice a file replaced in place.
const HEAD_BYTES: usize = 256;

/// One followed file. `T` is what a caller keeps from each span (`None` for a
/// line that isn't a span, or a span it doesn't need, so line counts match).
pub struct SpanTail<T> {
    path: PathBuf,
    /// Bytes read so far (always just past a newline).
    offset: u64,
    head: Vec<u8>,
    /// `(line bytes, kept)`, oldest first.
    lines: VecDeque<(u64, Option<T>)>,
    bytes: u64,
    cap: usize,
    keep: fn(Span) -> Option<T>,
}

impl<T: Clone> SpanTail<T> {
    /// Follow `path`, keeping at most `cap` lines.
    pub fn new(path: PathBuf, cap: usize, keep: fn(Span) -> Option<T>) -> Self {
        Self { path, offset: 0, head: Vec::new(), lines: VecDeque::new(), bytes: 0, cap, keep }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.head.clear();
        self.lines.clear();
        self.bytes = 0;
    }

    fn read_head(&self) -> Vec<u8> {
        let mut head = vec![0u8; HEAD_BYTES];
        let n = fs::File::open(&self.path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        head.truncate(n);
        head
    }

    /// Read what was appended since the last call.
    pub fn refresh(&mut self) {
        let Ok(len) = fs::metadata(&self.path).map(|m| m.len()) else {
            self.reset();
            return;
        };
        if len < self.offset || (self.offset > 0 && !self.read_head().starts_with(&self.head[..self.head.len().min(len as usize)])) {
            self.reset();
        }
        if len == self.offset {
            return;
        }
        let Ok(mut file) = fs::File::open(&self.path) else {
            self.reset();
            return;
        };
        // A first read starts at most SPAN_TAIL_BYTES from the end, like read_spans_tail.
        let fresh = self.offset == 0;
        let start = if fresh { len.saturating_sub(SPAN_TAIL_BYTES) } else { self.offset };
        if file.seek(SeekFrom::Start(start)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.take(len - start).read_to_end(&mut buf).is_err() {
            return;
        }
        // Only whole lines: a line still being written waits for the next read.
        let Some(end) = buf.iter().rposition(|b| *b == b'\n') else {
            return;
        };
        let mut chunk = &buf[..=end];
        if fresh && start > 0 {
            // The first line is cut mid-way by the seek.
            match chunk.iter().position(|b| *b == b'\n') {
                Some(cut) => chunk = &chunk[cut + 1..],
                None => chunk = &[],
            }
        }
        for raw in chunk.split_inclusive(|b| *b == b'\n') {
            let text = String::from_utf8_lossy(raw);
            let line = text.trim();
            if line.is_empty() {
                continue;
            }
            let kept = serde_json::from_str::<Span>(line).ok().and_then(self.keep);
            self.lines.push_back((raw.len() as u64, kept));
            self.bytes += raw.len() as u64;
        }
        while self.lines.len() > self.cap || self.bytes > SPAN_TAIL_BYTES {
            match self.lines.pop_front() {
                Some((n, _)) => self.bytes -= n,
                None => break,
            }
        }
        if fresh {
            self.head = self.read_head();
        }
        self.offset = start + end as u64 + 1;
    }

    /// What was kept from the newest `max_lines` lines, oldest first, borrowed.
    pub fn newest_iter(&self, max_lines: usize) -> impl Iterator<Item = &T> {
        let skip = self.lines.len().saturating_sub(max_lines);
        self.lines.iter().skip(skip).filter_map(|(_, kept)| kept.as_ref())
    }

    /// What was kept from the newest `max_lines` lines, oldest first.
    pub fn newest(&self, max_lines: usize) -> Vec<T> {
        self.newest_iter(max_lines).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{append_span, read_spans_tail, span_path, test_dir, AccessMode};

    fn span(session: &str, tool: &str, result: &str) -> Span {
        Span::soft_allow(session, tool, "{}", result, "", AccessMode::Supervised, "native")
    }

    fn tool_of(s: Span) -> Option<String> {
        Some(format!("{}:{}", s.tool, s.result))
    }

    #[test]
    fn it_follows_appends_and_matches_read_spans_tail() {
        let dir = test_dir("span-tail-follow");
        let path = span_path(&dir, "chat-1");
        let mut tail = SpanTail::new(path.clone(), 5, tool_of);
        tail.refresh();
        assert!(tail.newest(5).is_empty(), "no file yet");
        for i in 0..4 {
            append_span(&dir, &span("chat-1", "read", &i.to_string())).unwrap();
        }
        tail.refresh();
        assert_eq!(tail.newest(5), ["read:0", "read:1", "read:2", "read:3"]);
        for i in 4..8 {
            append_span(&dir, &span("chat-1", "read", &i.to_string())).unwrap();
        }
        // A half-written line waits; a non-span line still counts as a line.
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, b"not a span\n{\"partial\":").unwrap();
        tail.refresh();
        assert_eq!(tail.newest(5), ["read:4", "read:5", "read:6", "read:7"], "the cap keeps the newest 5 lines");
        assert_eq!(tail.newest(2), ["read:7"]);
        std::io::Write::write_all(&mut f, b"\"x\"}\n").unwrap();
        tail.refresh();
        assert_eq!(tail.newest(5), ["read:5", "read:6", "read:7"], "the finished line counts now");
        let (direct, _) = read_spans_tail(&dir, "chat-1", 5);
        assert_eq!(direct.into_iter().filter_map(tool_of).collect::<Vec<_>>(), tail.newest(5));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_shrank_or_was_replaced_is_read_again() {
        let dir = test_dir("span-tail-reset");
        let path = span_path(&dir, "chat-2");
        for i in 0..3 {
            append_span(&dir, &span("chat-2", "read", &i.to_string())).unwrap();
        }
        let mut tail = SpanTail::new(path.clone(), 100, tool_of);
        tail.refresh();
        assert_eq!(tail.newest(100).len(), 3);
        fs::remove_file(&path).unwrap();
        append_span(&dir, &span("chat-2", "write", "a")).unwrap();
        tail.refresh();
        assert_eq!(tail.newest(100), ["write:a"], "shorter file: read from scratch");
        // Same length or longer, different content: the head check catches it.
        let old = fs::read_to_string(&path).unwrap();
        let swapped = old.replace("\"write\"", "\"wrote\"");
        fs::write(&path, format!("{swapped}{swapped}")).unwrap();
        tail.refresh();
        assert_eq!(tail.newest(100), ["wrote:a", "wrote:a"]);
        let _ = fs::remove_dir_all(dir);
    }
}
