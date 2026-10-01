//! Minimal, bounded ZIP reader/writer for `.dotl` containers.
//!
//! Supported: stored (0) and deflate (8) entries, UTF-8 names, data descriptors.
//! Rejected with typed errors: encryption, ZIP64, multi-disk archives, other
//! compression methods, absolute or parent-relative paths, backslashes, duplicate
//! names, more entries/bytes than the limits allow, suspicious compression ratios,
//! CRC mismatches and size mismatches. Every offset and length is bounds-checked;
//! malformed input never panics.

use std::collections::BTreeSet;

use thiserror::Error;

/// Container limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLimits {
    /// Maximum total uncompressed size.
    pub max_total: u64,
    /// Maximum uncompressed size of one entry.
    pub max_entry: u64,
    /// Maximum number of entries.
    pub max_entries: usize,
    /// Maximum uncompressed/compressed ratio for deflated entries.
    pub max_ratio: u64,
}

impl Default for ZipLimits {
    fn default() -> Self {
        // Deflate cannot exceed ~1032:1; the size limits plus inflation bounded by the
        // declared size are the real protection, the ratio check rejects absurd headers.
        Self { max_total: 256 << 20, max_entry: 64 << 20, max_entries: 10_000, max_ratio: 1024 }
    }
}

/// ZIP errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ZipError {
    /// Not a ZIP archive or truncated.
    #[error("not a valid zip archive: {0}")]
    Malformed(&'static str),
    /// Unsupported feature.
    #[error("unsupported zip feature: {0}")]
    Unsupported(&'static str),
    /// Unsafe entry name.
    #[error("unsafe entry name `{0}`")]
    UnsafeName(String),
    /// Duplicate entry.
    #[error("duplicate entry `{0}`")]
    Duplicate(String),
    /// A limit was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(String),
    /// Data corrupt.
    #[error("corrupt entry `{0}`: {1}")]
    Corrupt(String, &'static str),
}

/// One archive entry (metadata).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Name.
    pub name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    /// Uncompressed size.
    pub size: u64,
    header_offset: u64,
}

/// A parsed archive borrowing the input bytes.
#[derive(Debug, Clone)]
pub struct Archive<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
    limits: ZipLimits,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// CRC-32 (IEEE) of `data`.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c = CRC_TABLE[((c ^ u32::from(*b)) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// Validate an entry name: relative, `/`-separated, no `..`, no drive letters.
pub fn check_name(name: &str) -> Result<(), ZipError> {
    let bad = || ZipError::UnsafeName(name.to_owned());
    if name.is_empty() || name.len() > 512 || name.starts_with('/') || name.contains('\\') || name.contains('\0') {
        return Err(bad());
    }
    if name.len() >= 2 && name.as_bytes().get(1) == Some(&b':') {
        return Err(bad());
    }
    for seg in name.split('/') {
        if seg == ".." || seg == "." || (seg.is_empty() && !name.ends_with('/')) {
            return Err(bad());
        }
    }
    if name.chars().any(char::is_control) {
        return Err(bad());
    }
    Ok(())
}

impl<'a> Archive<'a> {
    /// Parse the central directory.
    pub fn parse(data: &'a [u8], limits: ZipLimits) -> Result<Self, ZipError> {
        if data.len() < 22 {
            return Err(ZipError::Malformed("too short"));
        }
        // End of central directory: search the last 64 KiB + 22 bytes.
        let start = data.len().saturating_sub(0xFFFF + 22);
        let mut eocd = None;
        let mut i = data.len() - 22;
        loop {
            if u32_at(data, i) == Some(0x0605_4b50) {
                let comment_len = u16_at(data, i + 20).ok_or(ZipError::Malformed("eocd"))? as usize;
                if i + 22 + comment_len == data.len() {
                    eocd = Some(i);
                    break;
                }
            }
            if i == start {
                break;
            }
            i -= 1;
        }
        let e = eocd.ok_or(ZipError::Malformed("end of central directory not found"))?;
        let disk = u16_at(data, e + 4).ok_or(ZipError::Malformed("eocd"))?;
        let cd_disk = u16_at(data, e + 6).ok_or(ZipError::Malformed("eocd"))?;
        let n_disk = u16_at(data, e + 8).ok_or(ZipError::Malformed("eocd"))?;
        let n_total = u16_at(data, e + 10).ok_or(ZipError::Malformed("eocd"))?;
        let cd_size = u32_at(data, e + 12).ok_or(ZipError::Malformed("eocd"))?;
        let cd_off = u32_at(data, e + 16).ok_or(ZipError::Malformed("eocd"))?;
        if disk != 0 || cd_disk != 0 || n_disk != n_total {
            return Err(ZipError::Unsupported("multi-disk archive"));
        }
        if n_total == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF {
            return Err(ZipError::Unsupported("zip64"));
        }
        if usize::from(n_total) > limits.max_entries {
            return Err(ZipError::Limit(format!("{n_total} entries > {}", limits.max_entries)));
        }
        let cd_start = cd_off as usize;
        let cd_end = cd_start.checked_add(cd_size as usize).ok_or(ZipError::Malformed("central directory"))?;
        if cd_end > e {
            return Err(ZipError::Malformed("central directory out of bounds"));
        }
        let mut p = cd_start;
        let mut entries = Vec::with_capacity(usize::from(n_total));
        let mut names = BTreeSet::new();
        let mut total: u64 = 0;
        for _ in 0..n_total {
            if u32_at(data, p) != Some(0x0201_4b50) {
                return Err(ZipError::Malformed("bad central directory header"));
            }
            let flags = u16_at(data, p + 8).ok_or(ZipError::Malformed("cd"))?;
            let method = u16_at(data, p + 10).ok_or(ZipError::Malformed("cd"))?;
            let crc = u32_at(data, p + 16).ok_or(ZipError::Malformed("cd"))?;
            let compressed = u32_at(data, p + 20).ok_or(ZipError::Malformed("cd"))?;
            let size = u32_at(data, p + 24).ok_or(ZipError::Malformed("cd"))?;
            let name_len = u16_at(data, p + 28).ok_or(ZipError::Malformed("cd"))? as usize;
            let extra_len = u16_at(data, p + 30).ok_or(ZipError::Malformed("cd"))? as usize;
            let comment_len = u16_at(data, p + 32).ok_or(ZipError::Malformed("cd"))? as usize;
            let disk_start = u16_at(data, p + 34).ok_or(ZipError::Malformed("cd"))?;
            let header_offset = u32_at(data, p + 42).ok_or(ZipError::Malformed("cd"))?;
            if flags & 0x0001 != 0 || flags & 0x0040 != 0 {
                return Err(ZipError::Unsupported("encrypted entry"));
            }
            if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF || header_offset == 0xFFFF_FFFF {
                return Err(ZipError::Unsupported("zip64 entry"));
            }
            if disk_start != 0 {
                return Err(ZipError::Unsupported("multi-disk archive"));
            }
            if method != 0 && method != 8 {
                return Err(ZipError::Unsupported("compression method other than stored/deflate"));
            }
            let name_bytes = data.get(p + 46..p + 46 + name_len).ok_or(ZipError::Malformed("entry name"))?;
            let name = core::str::from_utf8(name_bytes)
                .map_err(|_| ZipError::UnsafeName(String::from_utf8_lossy(name_bytes).into_owned()))?;
            check_name(name)?;
            if !names.insert(name.to_owned()) {
                return Err(ZipError::Duplicate(name.to_owned()));
            }
            let (size, compressed) = (u64::from(size), u64::from(compressed));
            if size > limits.max_entry {
                return Err(ZipError::Limit(format!("entry `{name}` is {size} bytes")));
            }
            if method == 8 && compressed > 0 && size / compressed.max(1) > limits.max_ratio {
                return Err(ZipError::Limit(format!("entry `{name}` compression ratio too high")));
            }
            if method == 0 && compressed != size {
                return Err(ZipError::Corrupt(name.to_owned(), "stored entry size mismatch"));
            }
            total = total.saturating_add(size);
            if total > limits.max_total {
                return Err(ZipError::Limit(format!("total uncompressed size exceeds {} bytes", limits.max_total)));
            }
            entries.push(Entry {
                name: name.to_owned(),
                method,
                crc,
                compressed,
                size,
                header_offset: u64::from(header_offset),
            });
            p = p + 46 + name_len + extra_len + comment_len;
            if p > cd_end {
                return Err(ZipError::Malformed("central directory overflow"));
            }
        }
        Ok(Self { data, entries, limits })
    }

    /// Entries.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Entry by name.
    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Read and verify one entry.
    pub fn read(&self, e: &Entry) -> Result<Vec<u8>, ZipError> {
        let corrupt = |why| ZipError::Corrupt(e.name.clone(), why);
        let h = usize::try_from(e.header_offset).map_err(|_| corrupt("offset"))?;
        if u32_at(self.data, h) != Some(0x0403_4b50) {
            return Err(corrupt("bad local header"));
        }
        let name_len = u16_at(self.data, h + 26).ok_or_else(|| corrupt("local header"))? as usize;
        let extra_len = u16_at(self.data, h + 28).ok_or_else(|| corrupt("local header"))? as usize;
        let start = h + 30 + name_len + extra_len;
        let end = start
            .checked_add(usize::try_from(e.compressed).map_err(|_| corrupt("size"))?)
            .ok_or_else(|| corrupt("size"))?;
        let raw = self.data.get(start..end).ok_or_else(|| corrupt("data out of bounds"))?;
        let out = match e.method {
            0 => raw.to_vec(),
            8 => {
                let limit = usize::try_from(e.size.min(self.limits.max_entry)).map_err(|_| corrupt("size"))?;
                miniz_oxide::inflate::decompress_to_vec_with_limit(raw, limit)
                    .map_err(|_| corrupt("inflate failed or larger than declared"))?
            }
            _ => return Err(ZipError::Unsupported("compression method")),
        };
        if out.len() as u64 != e.size {
            return Err(corrupt("size mismatch"));
        }
        if crc32(&out) != e.crc {
            return Err(corrupt("crc mismatch"));
        }
        Ok(out)
    }
}

/// Write an archive. Entries are deflated when that makes them smaller.
pub fn write(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, ZipError> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let n = u16::try_from(entries.len()).map_err(|_| ZipError::Limit("too many entries".into()))?;
    for (name, data) in entries {
        check_name(name)?;
        let crc = crc32(data);
        let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
        let (method, payload): (u16, &[u8]) = if deflated.len() < data.len() { (8, &deflated) } else { (0, data) };
        let offset = u32::try_from(out.len()).map_err(|_| ZipError::Unsupported("archive larger than 4 GiB"))?;
        let csize = u32::try_from(payload.len()).map_err(|_| ZipError::Unsupported("entry larger than 4 GiB"))?;
        let usize_ = u32::try_from(data.len()).map_err(|_| ZipError::Unsupported("entry larger than 4 GiB"))?;
        let name_len = u16::try_from(name.len()).map_err(|_| ZipError::UnsafeName(name.clone()))?;
        // Local file header (UTF-8 flag set, fixed timestamp 1980-01-01 for reproducibility).
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x0021u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&csize.to_le_bytes());
        out.extend_from_slice(&usize_.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(payload);
        // Central directory record.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x0021u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&csize.to_le_bytes());
        central.extend_from_slice(&usize_.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&[0u8; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_off = u32::try_from(out.len()).map_err(|_| ZipError::Unsupported("archive larger than 4 GiB"))?;
    let cd_size = u32::try_from(central.len()).map_err(|_| ZipError::Unsupported("central directory too large"))?;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn roundtrip() {
        let big = "dotloom ".repeat(1000).into_bytes();
        let z = write(&[("a.json".into(), b"{}".to_vec()), ("assets/b.bin".into(), big.clone())]).unwrap();
        let a = Archive::parse(&z, ZipLimits::default()).unwrap();
        assert_eq!(a.entries().len(), 2);
        assert_eq!(a.read(a.entry("assets/b.bin").unwrap()).unwrap(), big);
        assert_eq!(a.read(a.entry("a.json").unwrap()).unwrap(), b"{}");
    }

    #[test]
    fn names_are_checked() {
        for bad in ["../x", "/abs", "a/../b", "C:/x", "a\\b", "", "a//b", "./a"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_name("assets/ab12.png").is_ok());
        assert!(write(&[("../evil".into(), vec![1])]).is_err());
    }

    #[test]
    fn bombs_and_limits() {
        let zeros = vec![0u8; 1 << 20];
        let z = write(&[("z".into(), zeros)]).unwrap();
        // 1 MiB of zeros deflates far beyond a 100:1 ratio.
        let strict = ZipLimits { max_ratio: 100, ..ZipLimits::default() };
        assert!(matches!(Archive::parse(&z, strict), Err(ZipError::Limit(_))));
        let ok = ZipLimits { max_ratio: u64::MAX, ..ZipLimits::default() };
        assert!(Archive::parse(&z, ok).is_ok());
        let small = ZipLimits { max_entry: 1000, max_ratio: u64::MAX, ..ZipLimits::default() };
        assert!(matches!(Archive::parse(&z, small), Err(ZipError::Limit(_))));
        let few = ZipLimits { max_entries: 0, ..ZipLimits::default() };
        assert!(matches!(Archive::parse(&z, few), Err(ZipError::Limit(_))));
    }

    #[test]
    fn corruption_is_detected() {
        let mut z = write(&[("a.txt".into(), b"hello world, hello world".to_vec())]).unwrap();
        // Flip a payload byte (after the 30-byte header + 5-byte name).
        z[36] ^= 0xff;
        let a = Archive::parse(&z, ZipLimits::default()).unwrap();
        assert!(a.read(&a.entries()[0]).is_err());
        assert!(Archive::parse(b"PK\x05\x06", ZipLimits::default()).is_err());
        assert!(Archive::parse(&[0u8; 100], ZipLimits::default()).is_err());
    }
}
