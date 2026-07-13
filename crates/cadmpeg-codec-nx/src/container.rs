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
    /// Locate independently size-framed NX object-model sections.
    pub fn om_sections(&self) -> Vec<(&DirEntry, crate::om::Section<'_>)> {
        let mut out = Vec::new();
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
            out.extend(
                crate::om::sections(payload)
                    .into_iter()
                    .map(|section| (entry, section)),
            );
        }
        out
    }

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
        self.external_reference_strings()
            .into_iter()
            .map(|(_, _, path)| path)
            .collect()
    }

    /// Extract child-part strings with their owning entry and payload offset.
    pub(crate) fn external_reference_strings(&self) -> Vec<(&DirEntry, usize, String)> {
        self.entries
            .iter()
            .filter(|entry| entry.name.contains("ExternalReferences"))
            .filter_map(|entry| entry.file_span.map(|span| (entry, span)))
            .flat_map(|(entry, (offset, size))| {
                let Ok(offset) = usize::try_from(offset) else {
                    return Vec::new();
                };
                let Ok(size) = usize::try_from(size) else {
                    return Vec::new();
                };
                self.data
                    .get(offset..offset.saturating_add(size))
                    .and_then(parse_extref_string_table)
                    .map(|(_, strings)| {
                        strings
                            .into_iter()
                            .map(|(relative, value)| (entry, relative, value))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Decode indexed EXTREFSTREAM record prefixes and sorted handle sets.
    pub(crate) fn external_reference_records(&self) -> Vec<(&DirEntry, ExtrefRecord)> {
        self.entries
            .iter()
            .filter(|entry| entry.name.contains("ExternalReferences"))
            .filter_map(|entry| {
                let (offset, size) = entry.file_span?;
                let (offset, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
                let payload = self.data.get(offset..offset.checked_add(size)?)?;
                Some(
                    parse_extref_records(payload)
                        .into_iter()
                        .map(move |record| (entry, record)),
                )
            })
            .flatten()
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

/// Decoded prefix of one indexed EXTREFSTREAM record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtrefRecord {
    pub record_id: u32,
    pub offset: usize,
    pub declared_count: u16,
    pub id_slots: [u32; 4],
    pub handles: Vec<u32>,
    pub closing_duplicate: bool,
    pub prefix_byte_len: usize,
    pub tail_byte_len: usize,
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(crate) fn parse_extref_string_table(payload: &[u8]) -> Option<(usize, Vec<(usize, String)>)> {
    (0..payload.len().saturating_sub(4))
        .rev()
        .find_map(|marker| {
            (payload[marker] == 1).then_some(())?;
            let count = u32_le(payload, marker + 1)? as usize;
            let mut pos = marker + 5;
            (count <= payload.len().saturating_sub(pos) / 3).then_some(())?;
            let mut out = Vec::with_capacity(count);
            for _ in 0..count {
                let raw_length = payload.get(pos..pos + 2)?;
                let length = usize::from(u16::from_le_bytes([raw_length[0], raw_length[1]]));
                let string_offset = pos + 2;
                pos = string_offset.checked_add(length)?;
                let raw = payload.get(string_offset..pos)?;
                let value = std::str::from_utf8(raw).ok()?;
                (!value.is_empty() && value.chars().all(|character| !character.is_control()))
                    .then_some(())?;
                out.push((string_offset, value.to_string()));
            }
            (pos == payload.len()).then_some((marker, out))
        })
}

pub(crate) fn parse_extref_records(payload: &[u8]) -> Vec<ExtrefRecord> {
    if !payload.starts_with(b"EXTREFSTREAM") || payload.get(24) != Some(&0) {
        return Vec::new();
    }
    let Some((string_table, _)) = parse_extref_string_table(payload) else {
        return Vec::new();
    };
    let mut directory = Vec::new();
    let mut at = 25usize;
    loop {
        let Some(record_id) = u32_le(payload, at) else {
            return Vec::new();
        };
        at += 4;
        if record_id == 0 {
            break;
        }
        let Some(offset) = u32_le(payload, at) else {
            return Vec::new();
        };
        at += 4;
        let offset = offset as usize;
        if offset >= string_table {
            return Vec::new();
        }
        directory.push((record_id, offset));
    }
    if directory.is_empty()
        || !directory.windows(2).all(|pair| pair[0].1 < pair[1].1)
        || at > directory[0].1
    {
        return Vec::new();
    }

    let parse_record = |record_id, offset, end| -> Option<ExtrefRecord> {
        let bytes = payload.get(offset..end)?;
        (bytes.get(..4) == Some(&[1, 0, 0, 0]) && bytes.get(6) == Some(&1)).then_some(())?;
        let declared_count = u16::from_be_bytes(bytes.get(4..6)?.try_into().ok()?);
        let mut id_slots = [0; 4];
        for (slot, value) in id_slots.iter_mut().enumerate() {
            *value = u32::from_le_bytes(bytes.get(7 + slot * 4..11 + slot * 4)?.try_into().ok()?);
        }
        (bytes.get(23) == Some(&1)).then_some(())?;
        let count = usize::from(*bytes.get(24)?);
        (count >= 2).then_some(())?;
        let handle_token_count = count - 1;
        let prefix_byte_len = 26usize.checked_add(handle_token_count.checked_mul(5)?)?;
        (prefix_byte_len <= bytes.len() && bytes.get(prefix_byte_len - 1) == Some(&(count as u8)))
            .then_some(())?;
        let mut handles = Vec::with_capacity(handle_token_count);
        for handle_index in 0..handle_token_count {
            let token = 25 + handle_index * 5;
            (bytes.get(token) == Some(&0xe0)).then_some(())?;
            handles.push(u32::from_be_bytes(
                bytes.get(token + 1..token + 5)?.try_into().ok()?,
            ));
        }
        let closing_duplicate = handle_token_count >= 2
            && handles[handle_token_count - 1] == handles[handle_token_count - 2];
        let unique_count = handle_token_count - usize::from(closing_duplicate);
        handles[..unique_count]
            .windows(2)
            .all(|pair| pair[0] < pair[1])
            .then_some(())?;
        if closing_duplicate {
            handles.pop();
        }
        Some(ExtrefRecord {
            record_id,
            offset,
            declared_count,
            id_slots,
            handles,
            closing_duplicate,
            prefix_byte_len,
            tail_byte_len: bytes.len() - prefix_byte_len,
        })
    };

    let mut records = Vec::with_capacity(directory.len());
    for (index, (record_id, offset)) in directory.iter().copied().enumerate() {
        let end = directory
            .get(index + 1)
            .map_or(string_table, |(_, offset)| *offset);
        if let Some(record) = parse_record(record_id, offset, end) {
            records.push(record);
        }
    }
    records
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
