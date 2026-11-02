//! Append-only event store: hourly-ish segment files, each a zstd stream of
//! length-prefixed bincode frames (one frame = all events of one block, in order).
//! Crash-safe enough: a truncated tail frame is ignored on read.

use crate::event::Ev;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

pub struct StoreWriter {
    dir: PathBuf,
    rotate_slots: u64,
    cur: Option<(u64, zstd::stream::write::Encoder<'static, BufWriter<File>>)>,
    frames_since_flush: u32,
    last_slot: u64,
    level: i32,
}

impl StoreWriter {
    pub fn new(dir: impl AsRef<Path>, rotate_slots: u64) -> Result<Self> {
        fs::create_dir_all(dir.as_ref())?;
        Ok(StoreWriter {
            dir: dir.as_ref().to_path_buf(),
            rotate_slots,
            cur: None,
            frames_since_flush: 0,
            last_slot: 0,
            level: 6,
        })
    }

    /// Same, with a custom zstd level (archival imports use a higher one).
    pub fn with_level(dir: impl AsRef<Path>, rotate_slots: u64, level: i32) -> Result<Self> {
        let mut w = Self::new(dir, rotate_slots)?;
        w.level = level;
        Ok(w)
    }

    fn open(&mut self, slot: u64) -> Result<()> {
        self.close()?;
        let mut path = self.dir.join(format!("ev_{slot:012}.zst"));
        let mut n = 1;
        while path.exists() {
            path = self.dir.join(format!("ev_{slot:012}_{n}.zst"));
            n += 1;
        }
        let f = BufWriter::with_capacity(1 << 20, File::create(&path)?);
        let enc = zstd::stream::write::Encoder::new(f, self.level)?;
        self.cur = Some((slot, enc));
        Ok(())
    }

    /// Write all events of one block (must be called in increasing slot order).
    pub fn write_block(&mut self, slot: u64, evs: &[Ev]) -> Result<()> {
        let need_new = match &self.cur {
            None => true,
            Some((start, _)) => slot >= start + self.rotate_slots,
        };
        if need_new {
            self.open(slot)?;
        }
        let bytes = bincode::serialize(evs)?;
        let (_, enc) = self.cur.as_mut().unwrap();
        enc.write_all(&(bytes.len() as u32).to_le_bytes())?;
        enc.write_all(&bytes)?;
        self.last_slot = slot;
        self.frames_since_flush += 1;
        if self.frames_since_flush >= 64 {
            enc.flush()?;
            self.frames_since_flush = 0;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        if let Some((_, enc)) = self.cur.as_mut() {
            enc.flush()?;
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        if let Some((_, enc)) = self.cur.take() {
            let mut w = enc.finish()?;
            w.flush()?;
        }
        Ok(())
    }

    pub fn last_slot(&self) -> u64 {
        self.last_slot
    }
}

impl Drop for StoreWriter {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// List segment files sorted by starting slot.
pub fn segments(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir.as_ref())
        .with_context(|| format!("reading {}", dir.as_ref().display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ev_") && n.ends_with(".zst"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    Ok(v)
}

/// Iterate blocks (Vec<Ev>) across all segments, in file order.
pub struct StoreReader {
    files: Vec<PathBuf>,
    next_file: usize,
    cur: Option<zstd::stream::read::Decoder<'static, BufReader<File>>>,
    pub frames_read: u64,
    pub truncated_frames: u64,
}

impl StoreReader {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        Ok(StoreReader {
            files: segments(dir)?,
            next_file: 0,
            cur: None,
            frames_read: 0,
            truncated_frames: 0,
        })
    }

    fn next_frame(&mut self) -> Option<Vec<Ev>> {
        loop {
            if self.cur.is_none() {
                let f = self.files.get(self.next_file)?;
                self.next_file += 1;
                match File::open(f).and_then(zstd::stream::read::Decoder::new) {
                    Ok(d) => self.cur = Some(d),
                    Err(_) => continue,
                }
            }
            let dec = self.cur.as_mut().unwrap();
            let mut len = [0u8; 4];
            if dec.read_exact(&mut len).is_err() {
                self.cur = None;
                continue;
            }
            let n = u32::from_le_bytes(len) as usize;
            let mut buf = vec![0u8; n];
            if dec.read_exact(&mut buf).is_err() {
                self.truncated_frames += 1;
                self.cur = None;
                continue;
            }
            match bincode::deserialize::<Vec<Ev>>(&buf) {
                Ok(v) => {
                    self.frames_read += 1;
                    return Some(v);
                }
                Err(_) => {
                    self.truncated_frames += 1;
                    self.cur = None;
                    continue;
                }
            }
        }
    }
}

impl Iterator for StoreReader {
    type Item = Vec<Ev>;
    fn next(&mut self) -> Option<Vec<Ev>> {
        self.next_frame()
    }
}
