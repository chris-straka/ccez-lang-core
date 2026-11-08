//! The offline dictionaries as the phone reads them: ccez-lang's files
//! (sorted `key<TAB>json` lines, gzipped, or CC-CEDICT's own lines)
//! packed once, on the phone, into blocks of 256 lines, each deflated on
//! its own, with an index of each block's first key. A lookup inflates
//! one block; the last few stay inflated.
//!
//! Layout: `CZDPACK1`, the blocks, the index (count, then per block a
//! u16 key length, the key, u64 offset, u32 length), and the index's
//! offset as the last 8 bytes. Little-endian. Keys sort by bytes, so
//! the packer sorts again: ccez-lang's files are in JavaScript's order.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use flate2::Compression;
use flate2::read::{DeflateDecoder, GzDecoder};
use flate2::write::DeflateEncoder;

const MAGIC: &[u8; 8] = b"CZDPACK1";
const BLOCK_LINES: usize = 256;
const CACHED_BLOCKS: usize = 48;
/// Joins several CC-CEDICT lines under one key.
pub const SEP: char = '\u{1f}';

type Block = Arc<Vec<(String, String)>>;

pub struct Pack {
    file: File,
    /// First key, offset, length of each block.
    index: Vec<(String, u64, u32)>,
    cache: Mutex<VecDeque<(usize, Block)>>,
}

/// A file's text, gunzipped when it is gzip (Android's packager may
/// have unpacked it already).
fn open_text(path: &Path) -> io::Result<Box<dyn BufRead>> {
    let mut file = BufReader::new(File::open(path)?);
    let gzip = file.fill_buf()?.starts_with(&[0x1f, 0x8b]);
    Ok(if gzip { Box::new(BufReader::new(GzDecoder::new(file))) } else { Box::new(file) })
}

/// A `key<TAB>value` file's lines.
pub fn read_tsv_gz(path: &Path) -> io::Result<Vec<(String, String)>> {
    let reader = open_text(path)?;
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if let Some((k, v)) = line.split_once('\t') {
            out.push((k.to_string(), v.to_string()));
        }
    }
    Ok(out)
}

/// CC-CEDICT, gzipped, keyed by both its simplified and traditional
/// forms; a key with several entries keeps their lines joined by [`SEP`].
pub fn read_cedict_gz(path: &Path) -> io::Result<Vec<(String, String)>> {
    let reader = open_text(path)?;
    let mut by_key: HashMap<String, String> = HashMap::new();
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let (Some(trad), Some(simp)) = (parts.next(), parts.next()) else {
            continue;
        };
        let keys = if trad == simp { vec![trad] } else { vec![simp, trad] };
        for k in keys {
            let held = by_key.entry(k.to_string()).or_default();
            if !held.is_empty() {
                held.push(SEP);
            }
            held.push_str(&line);
        }
    }
    Ok(by_key.into_iter().collect())
}

/// Writes `entries` as a pack at `out` (through a temporary file, so a
/// pack that exists is whole).
pub fn pack(mut entries: Vec<(String, String)>, out: &Path) -> io::Result<()> {
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    let tmp = out.with_extension("tmp");
    let mut file = io::BufWriter::new(File::create(&tmp)?);
    file.write_all(MAGIC)?;
    let mut at = MAGIC.len() as u64;
    let mut index = Vec::new();
    for chunk in entries.chunks(BLOCK_LINES) {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        for (k, v) in chunk {
            enc.write_all(k.as_bytes())?;
            enc.write_all(b"\t")?;
            enc.write_all(v.as_bytes())?;
            enc.write_all(b"\n")?;
        }
        let body = enc.finish()?;
        file.write_all(&body)?;
        index.push((chunk[0].0.as_str(), at, body.len() as u32));
        at += body.len() as u64;
    }
    let index_at = at;
    file.write_all(&(index.len() as u32).to_le_bytes())?;
    for (key, off, len) in index {
        file.write_all(&(key.len() as u16).to_le_bytes())?;
        file.write_all(key.as_bytes())?;
        file.write_all(&off.to_le_bytes())?;
        file.write_all(&len.to_le_bytes())?;
    }
    file.write_all(&index_at.to_le_bytes())?;
    file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    std::fs::rename(tmp, out)
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("dictionary pack: {what}"))
}

impl Pack {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut magic = [0u8; 8];
        file.read_exact_at(&mut magic, 0)?;
        if &magic != MAGIC || size < 20 {
            return Err(bad("not a pack"));
        }
        let mut tail = [0u8; 8];
        file.read_exact_at(&mut tail, size - 8)?;
        let index_at = u64::from_le_bytes(tail);
        if index_at >= size - 8 {
            return Err(bad("index out of range"));
        }
        let mut raw = vec![0u8; (size - 8 - index_at) as usize];
        file.read_exact_at(&mut raw, index_at)?;
        let mut r = &raw[..];
        let count = take_u32(&mut r)? as usize;
        let mut index = Vec::with_capacity(count);
        for _ in 0..count {
            let len = take::<2>(&mut r).map(u16::from_le_bytes)? as usize;
            if r.len() < len {
                return Err(bad("index cut short"));
            }
            let key = String::from_utf8(r[..len].to_vec()).map_err(|_| bad("key not UTF-8"))?;
            r = &r[len..];
            let off = take::<8>(&mut r).map(u64::from_le_bytes)?;
            let blen = take_u32(&mut r)?;
            index.push((key, off, blen));
        }
        Ok(Self {
            file,
            index,
            cache: Mutex::new(VecDeque::new()),
        })
    }

    /// The value under `key`, if the dictionary has it.
    pub fn get(&self, key: &str) -> Option<String> {
        // The last block whose first key is at or before this one.
        let i = self.index.partition_point(|(first, _, _)| first.as_str() <= key);
        let block = self.block(i.checked_sub(1)?)?;
        let at = block.binary_search_by(|(k, _)| k.as_str().cmp(key)).ok()?;
        Some(block[at].1.clone())
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    fn block(&self, i: usize) -> Option<Block> {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((_, b)) = cache.iter().find(|(n, _)| *n == i) {
            return Some(b.clone());
        }
        let (_, off, len) = &self.index[i];
        let mut raw = vec![0u8; *len as usize];
        self.file.read_exact_at(&mut raw, *off).ok()?;
        let mut text = String::new();
        DeflateDecoder::new(&raw[..]).read_to_string(&mut text).ok()?;
        let lines: Vec<(String, String)> = text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let block = Arc::new(lines);
        if cache.len() >= CACHED_BLOCKS {
            cache.pop_front();
        }
        cache.push_back((i, block.clone()));
        Some(block)
    }
}

fn take<const N: usize>(r: &mut &[u8]) -> io::Result<[u8; N]> {
    if r.len() < N {
        return Err(bad("index cut short"));
    }
    let (head, rest) = r.split_at(N);
    *r = rest;
    Ok(head.try_into().expect("split at N"))
}

fn take_u32(r: &mut &[u8]) -> io::Result<u32> {
    take::<4>(r).map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_finds_every_key_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("ccez-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pack");
        // More than one block, out of byte order as JavaScript sorts
        // (an astral character before a BMP one above U+E000).
        let mut entries: Vec<(String, String)> =
            (0..1000).map(|i| (format!("w{i:04}"), format!("[{i}]"))).collect();
        entries.push(("𠮷".into(), "astral".into()));
        entries.push(("ｱ".into(), "halfwidth".into()));
        pack(entries, &path).unwrap();
        let p = Pack::open(&path).unwrap();
        assert_eq!(p.get("w0000").as_deref(), Some("[0]"));
        assert_eq!(p.get("w0257").as_deref(), Some("[257]"));
        assert_eq!(p.get("w0999").as_deref(), Some("[999]"));
        assert_eq!(p.get("𠮷").as_deref(), Some("astral"));
        assert_eq!(p.get("ｱ").as_deref(), Some("halfwidth"));
        assert_eq!(p.get("w1000"), None);
        assert_eq!(p.get("a"), None);
        assert_eq!(p.get(""), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
