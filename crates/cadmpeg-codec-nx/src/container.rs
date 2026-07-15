// SPDX-License-Identifier: Apache-2.0
//! Parse the SPLMSSTR header and its `HEADER` and `FOOTER` directories.
//!
//! An NX part begins with the eight-byte `SPLMSSTR` signature. Container integers
//! are little-endian. Directory entries name `/Root/...` paths and may carry an
//! in-bounds file offset and size. [`crate::parasolid`] uses the canonical
//! `/Root/UG_PART/UG_PART` span to bound its compressed-stream scan.

use cadmpeg_ir::codec::{CodecError, ReadSeek};
use cadmpeg_ir::le::{u32_at as u32_le, u64_at as u64_le};

/// The eight-byte signature used to identify an SPLMSSTR container.
pub const MAGIC: &[u8; 8] = b"SPLMSSTR";

/// A directory entry from the `HEADER` or `FOOTER` region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The `/Root/...` path name.
    pub name: String,
    /// Which region the entry was read from.
    pub region: Region,
    /// An in-bounds byte offset and length, or `None` for non-file entries.
    pub file_span: Option<(u64, u64)>,
}

/// Directory region containing an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// The `HEADER` region near the start of the file.
    Header,
    /// The `FOOTER` region near EOF.
    Footer,
}

impl Region {
    /// Return the directory-region label used in summaries.
    pub fn label(self) -> &'static str {
        match self {
            Region::Header => "HEADER",
            Region::Footer => "FOOTER",
        }
    }
}

impl Container {
    /// Locate indexed NX object-model sections in catalogued file entries.
    pub fn indexed_om_sections(&self) -> Vec<(&DirEntry, crate::om::IndexedSection<'_>)> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.entries {
            let Some((offset, size)) = entry.file_span else {
                continue;
            };
            let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
                continue;
            };
            let Some(payload) = self.data.get(offset..offset.saturating_add(size)) else {
                continue;
            };
            for section in crate::om::indexed_sections(payload) {
                if seen.insert((offset, section.object_id_table_offset)) {
                    out.push((entry, section));
                }
            }
        }
        out
    }

    /// Extract child-part paths from catalogued external-reference payloads.
    pub fn external_reference_paths(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.name.contains("ExternalReferences"))
            .filter_map(|entry| entry.file_span)
            .flat_map(|(offset, size)| {
                let Ok(offset) = usize::try_from(offset) else {
                    return Vec::new();
                };
                let Ok(size) = usize::try_from(size) else {
                    return Vec::new();
                };
                self.data
                    .get(offset..offset.saturating_add(size))
                    .map(parse_extref_paths)
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Extract active NX object identifiers from `/Root/FastLoad/RMFastLoad`.
    pub fn rmfastload_object_ids(&self) -> Vec<u32> {
        let Some((offset, size)) = self
            .entries
            .iter()
            .find(|entry| entry.name == "/Root/FastLoad/RMFastLoad")
            .and_then(|entry| entry.file_span)
        else {
            return Vec::new();
        };
        let Ok(offset) = usize::try_from(offset) else {
            return Vec::new();
        };
        let Ok(size) = usize::try_from(size) else {
            return Vec::new();
        };
        let Some(bytes) = self.data.get(offset..offset.saturating_add(size)) else {
            return Vec::new();
        };
        let Some(registry) = find(bytes, b"UGS::Solid::Topol") else {
            return Vec::new();
        };
        for pos in registry..bytes.len().saturating_sub(4) {
            let Some(count) = u32_le(bytes, pos).map(|value| value as usize) else {
                continue;
            };
            if !(50..=70_000).contains(&count) {
                continue;
            }
            let Some(raw) = bytes.get(pos + 4..pos + 4 + count * 4) else {
                continue;
            };
            let ids: Vec<_> = raw
                .chunks_exact(4)
                .map(|word| {
                    u32::from_le_bytes(
                        word.try_into()
                            .expect("invariant: chunks_exact(4) yields exactly 4-byte slices"),
                    )
                })
                .collect();
            if ids.iter().all(|id| (1..70_000).contains(id)) {
                return ids;
            }
        }
        Vec::new()
    }
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_extref_paths(payload: &[u8]) -> Vec<String> {
    let Some(marker) = (0..payload.len().saturating_sub(4))
        .rev()
        .find(|&offset| payload[offset] == 1 && u32_le(payload, offset + 1).is_some())
    else {
        return Vec::new();
    };
    let Some(mut cursor) =
        cadmpeg_ir::cursor::Cursor::with_bounds(payload, marker + 1, payload.len())
    else {
        return Vec::new();
    };
    let Some(count) = cursor.u32_le() else {
        return Vec::new();
    };
    // Each path is at least a u16 length prefix; an implausible declared
    // count fails here instead of reserving unbounded capacity.
    cursor
        .read_counted(u64::from(count), 2, |cursor| {
            let length = usize::from(cursor.u16_le()?);
            let raw = cursor.take(length)?;
            Some(std::str::from_utf8(raw).ok()?.to_string())
        })
        .unwrap_or_default()
}

/// A parsed SPLMSSTR container and its directory entries.
#[derive(Debug, Clone)]
pub struct Container {
    /// The whole file image.
    pub data: Vec<u8>,
    /// Version byte at file offset 8.
    pub version: u8,
    /// File-specific 24-bit little-endian value at offset 9.
    pub file_tag: u32,
    /// Offset of the `FOOTER` region.
    pub footer_offset: u64,
    /// Enumerated directory entries from both regions, in discovery order.
    pub entries: Vec<DirEntry>,
}

/// Return whether `prefix` starts with [`MAGIC`].
pub fn looks_like_nx(prefix: &[u8]) -> bool {
    prefix.starts_with(MAGIC)
}

fn u24_le(d: &[u8], at: usize) -> u32 {
    if at + 3 > d.len() {
        return 0;
    }
    u32::from(d[at]) | (u32::from(d[at + 1]) << 8) | (u32::from(d[at + 2]) << 16)
}

fn u48_le(d: &[u8], at: usize) -> u64 {
    let mut v = 0u64;
    for i in 0..6 {
        if at + i < d.len() {
            v |= u64::from(d[at + i]) << (8 * i);
        }
    }
    v
}

/// Read a complete SPLMSSTR file and parse its header and directories.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<Container, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(CodecError::Io)?;
    scan_bytes(data)
}

/// Parse an SPLMSSTR file image.
pub fn scan_bytes(data: Vec<u8>) -> Result<Container, CodecError> {
    if !data.starts_with(MAGIC) {
        return Err(CodecError::WrongFormat(
            "missing SPLMSSTR magic".to_string(),
        ));
    }
    let version = data.get(8).copied().unwrap_or(0);
    let file_tag = u24_le(&data, 9);
    let footer_offset = u48_le(&data, 0x11);

    let mut entries = Vec::new();
    // The HEADER directory begins at +25 (`0x19`). Scan forward from there for
    // entries until the first non-entry byte; the region is contiguous.
    enumerate_region(&data, 0x19, Region::Header, &mut entries);
    // The FOOTER region begins at the 48-bit offset with an ASCII `FOOTER` tag,
    // then a `u32 LE` entry count; entries follow.
    let fo = footer_offset as usize;
    if fo + 10 <= data.len() && &data[fo..fo + 6] == b"FOOTER" {
        enumerate_region(&data, fo + 10, Region::Footer, &mut entries);
    }

    Ok(Container {
        data,
        version,
        file_tag,
        footer_offset,
        entries,
    })
}

/// Walk a directory region starting at `from`, appending every entry whose
/// `name_len:u32 LE` frames an in-bounds ASCII `/Root/...` path. Stops at the
/// first position that does not frame such an entry (the region is contiguous).
fn enumerate_region(data: &[u8], from: usize, region: Region, out: &mut Vec<DirEntry>) {
    let mut o = from;
    // The very first HEADER entry is the `/Root/` sentinel; a run of well-formed
    // entries follows. Allow a bounded number of framing misses before giving up,
    // because the 16-byte opaque payloads can contain bytes that briefly look like
    // a length field.
    let mut misses = 0usize;
    while o + 4 <= data.len() && misses < 64 {
        match try_entry(data, o, region) {
            Some((entry, next)) => {
                out.push(entry);
                o = next;
                misses = 0;
            }
            None => {
                o += 1;
                misses += 1;
            }
        }
    }
}

/// Try to read one directory entry at `o`: `name_len:u32 LE`, then that many bytes
/// of printable ASCII beginning `/Root`, then a 16-byte payload. Returns the entry
/// and the offset just past its payload.
fn try_entry(data: &[u8], o: usize, region: Region) -> Option<(DirEntry, usize)> {
    let name_len = u32_le(data, o)? as usize;
    if !(6..=128).contains(&name_len) {
        return None;
    }
    let name_start = o + 4;
    let name_end = name_start + name_len;
    let raw = data.get(name_start..name_end)?;
    if !raw.starts_with(b"/Root") || !raw.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        return None;
    }
    let name = String::from_utf8_lossy(raw).into_owned();
    let payload = name_end;
    // Interpret the 16-byte payload as a file span when it lands within the file.
    let file_span = match (u64_le(data, payload), u64_le(data, payload + 8)) {
        (Some(off), Some(size)) => {
            let end = off.checked_add(size);
            match end {
                Some(e) if size > 0 && e <= data.len() as u64 && off >= 8 => Some((off, size)),
                _ => None,
            }
        }
        _ => None,
    };
    Some((
        DirEntry {
            name,
            region,
            file_span,
        },
        payload + 16,
    ))
}
