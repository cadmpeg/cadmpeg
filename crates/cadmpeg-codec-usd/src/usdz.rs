// SPDX-License-Identifier: Apache-2.0
//! USDZ packages (spec §1): a ZIP archive whose first member is the root
//! layer.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{CapacityField, EncodeError};

pub(crate) mod source;

/// The ZIP local file header signature.
const LOCAL_HEADER_MAGIC: &[u8; 4] = b"PK\x03\x04";
/// The file name extensions of a root layer.
const LAYER_EXTENSIONS: [&str; 3] = [".usda", ".usdc", ".usd"];
/// The alignment of member data in a package.
const DATA_ALIGNMENT: usize = 64;
/// The member name of the root layer this writer packages.
pub(crate) const ROOT_MEMBER: &str = "root.usda";

/// Whether `name` is the file name of a layer.
pub(crate) fn is_layer_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    LAYER_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// Whether the bytes start as a ZIP archive.
pub(crate) fn is_archive(prefix: &[u8]) -> bool {
    prefix.starts_with(LOCAL_HEADER_MAGIC)
}

/// The members of a package and its root layer.
#[derive(Debug)]
pub(crate) struct Package<'a> {
    pub(crate) snapshot: ArchiveSnapshot<'a>,
    /// The member name of the root layer.
    pub(crate) root_name: String,
    /// The bytes of the root layer.
    pub(crate) root: View<'a>,
}

/// Opens the package `archive` and its root layer.
pub(crate) fn open<'a>(
    ctx: &DecodeContext<'a>,
    archive: View<'a>,
) -> Result<Package<'a>, CodecError> {
    let snapshot = ArchiveSnapshot::new(ctx, archive)?;
    let first = snapshot
        .entries()
        .iter()
        .min_by_key(|entry| entry.header_start)
        .ok_or_else(|| CodecError::malformed("the USDZ package has no member"))?;
    if !is_layer_name(&first.name) {
        return Err(CodecError::WrongFormat(format!(
            "the first ZIP member `{}` is not a layer",
            first.name
        )));
    }
    let root_name = first.name.clone();
    let root = snapshot.open(ctx, &root_name)?;
    Ok(Package {
        snapshot,
        root_name,
        root,
    })
}

/// Whether `prefix` is a package whose first member is a layer.
pub(crate) fn detect(ctx: &DecodeContext<'_>, prefix: &[u8]) -> Result<bool, CodecError> {
    if !is_archive(prefix) && !ArchiveSnapshot::has_footer(prefix) {
        return Ok(false);
    }
    match ArchiveSnapshot::new(ctx, View::over_retained(prefix)) {
        Ok(snapshot) => Ok(snapshot
            .entries()
            .iter()
            .min_by_key(|entry| entry.header_start)
            .is_some_and(|entry| is_layer_name(&entry.name))),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(false),
    }
}

/// The member name `asset` names inside a package whose root layer is
/// `root_name`, when it is a relative path with no scheme.
pub(crate) fn member_name(root_name: &str, asset: &str) -> Option<String> {
    if asset.is_empty() || asset.starts_with('/') || asset.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = root_name.split('/').collect();
    parts.pop();
    for part in asset.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Appends a little-endian `u16`.
fn u16_le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a little-endian `u32`.
fn u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

const CAPACITY_FIELDS: &[CapacityField] = &[CapacityField {
    field: "zip.member.name_length",
    limit: 65_535,
    source: "docs/formats/zip.md §§1–2: member filename length is a 16-bit word",
    helper: "member_name_length",
}];

fn member_name_length(length: usize) -> Result<u16, EncodeError> {
    u16::try_from(length).map_err(|_| EncodeError::TargetCapacity {
        field: &CAPACITY_FIELDS[0],
        limit: CAPACITY_FIELDS[0].limit,
        value: u64_from_index(length),
    })
}

fn u64_le(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn zip32(value: u64) -> u32 {
    if value >= u64::from(u32::MAX) {
        u32::MAX
    } else {
        u32::try_from(value).unwrap_or_default()
    }
}

fn zip16(value: u64) -> u16 {
    if value >= u64::from(u16::MAX) {
        u16::MAX
    } else {
        u16::try_from(value).unwrap_or_default()
    }
}

/// A package of stored members, each with data aligned to 64 bytes.
pub(crate) fn package(members: &[(&str, &[u8])]) -> Result<Vec<u8>, EncodeError> {
    const VERSION: u16 = 10;
    const ZIP64_VERSION: u16 = 45;
    const LOCAL_HEADER_LEN: usize = 30;
    const EXTRA_HEADER_LEN: usize = 4;
    const PADDING_ID: u16 = 0x1986;
    const ZIP64_ID: u16 = 1;
    let mut out = Vec::new();
    let mut entries = Vec::new();
    let mut any_zip64 = false;
    for (name, data) in members {
        let name_len = member_name_length(name.len())?;
        let size = u64_from_index(data.len());
        let offset = u64_from_index(out.len());
        let large_size = size >= u64::from(u32::MAX);
        let large_offset = offset >= u64::from(u32::MAX);
        any_zip64 |= large_size || large_offset;
        let version = if large_size || large_offset {
            ZIP64_VERSION
        } else {
            VERSION
        };
        let local_zip64_len = if large_size { 20 } else { 0 };
        let unpadded = out.len() + LOCAL_HEADER_LEN + name.len() + local_zip64_len;
        let mut padding = (DATA_ALIGNMENT - unpadded % DATA_ALIGNMENT) % DATA_ALIGNMENT;
        if padding > 0 && padding < EXTRA_HEADER_LEN {
            padding += DATA_ALIGNMENT;
        }
        debug_assert!(
            u16::try_from(local_zip64_len + padding).is_ok(),
            "aligned ZIP extra fields fit their length word"
        );
        let extra_len = u16::try_from(local_zip64_len + padding).unwrap_or_default();
        let crc = crc32fast::hash(data);
        let size32 = zip32(size);
        out.extend_from_slice(LOCAL_HEADER_MAGIC);
        u16_le(&mut out, version);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, crc);
        u32_le(&mut out, size32);
        u32_le(&mut out, size32);
        u16_le(&mut out, name_len);
        u16_le(&mut out, extra_len);
        out.extend_from_slice(name.as_bytes());
        if large_size {
            u16_le(&mut out, ZIP64_ID);
            u16_le(&mut out, 16);
            u64_le(&mut out, size);
            u64_le(&mut out, size);
        }
        if padding > 0 {
            u16_le(&mut out, PADDING_ID);
            u16_le(
                &mut out,
                u16::try_from(padding - EXTRA_HEADER_LEN).unwrap_or_default(),
            );
            out.resize(out.len() + padding - EXTRA_HEADER_LEN, 0);
        }
        out.extend_from_slice(data);
        entries.push((name, name_len, size, crc, offset, version));
    }
    let count = u64_from_index(entries.len());
    let central_start = u64_from_index(out.len());
    for (name, name_len, size, crc, offset, version) in entries {
        let large_size = size >= u64::from(u32::MAX);
        let large_offset = offset >= u64::from(u32::MAX);
        let extra_values = usize::from(large_size) * 16 + usize::from(large_offset) * 8;
        let extra_len = if extra_values == 0 {
            0
        } else {
            extra_values + EXTRA_HEADER_LEN
        };
        debug_assert!(
            u16::try_from(extra_len).is_ok(),
            "ZIP64 central extra fields fit their length word"
        );
        out.extend_from_slice(b"PK\x01\x02");
        u16_le(&mut out, version);
        u16_le(&mut out, version);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, crc);
        let size32 = zip32(size);
        u32_le(&mut out, size32);
        u32_le(&mut out, size32);
        u16_le(&mut out, name_len);
        u16_le(&mut out, u16::try_from(extra_len).unwrap_or_default());
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, 0);
        u32_le(&mut out, zip32(offset));
        out.extend_from_slice(name.as_bytes());
        if extra_values > 0 {
            u16_le(&mut out, ZIP64_ID);
            u16_le(&mut out, u16::try_from(extra_values).unwrap_or_default());
            if large_size {
                u64_le(&mut out, size);
                u64_le(&mut out, size);
            }
            if large_offset {
                u64_le(&mut out, offset);
            }
        }
    }
    let central_size = u64_from_index(out.len()) - central_start;
    let zip64 = any_zip64
        || count >= u64::from(u16::MAX)
        || central_start >= u64::from(u32::MAX)
        || central_size >= u64::from(u32::MAX);
    if zip64 {
        let end_offset = u64_from_index(out.len());
        out.extend_from_slice(b"PK\x06\x06");
        u64_le(&mut out, 44);
        u16_le(&mut out, ZIP64_VERSION);
        u16_le(&mut out, ZIP64_VERSION);
        u32_le(&mut out, 0);
        u32_le(&mut out, 0);
        u64_le(&mut out, count);
        u64_le(&mut out, count);
        u64_le(&mut out, central_size);
        u64_le(&mut out, central_start);
        out.extend_from_slice(b"PK\x06\x07");
        u32_le(&mut out, 0);
        u64_le(&mut out, end_offset);
        u32_le(&mut out, 1);
    }
    out.extend_from_slice(b"PK\x05\x06");
    u16_le(&mut out, 0);
    u16_le(&mut out, 0);
    let count16 = zip16(count);
    u16_le(&mut out, count16);
    u16_le(&mut out, count16);
    u32_le(&mut out, zip32(central_size));
    u32_le(&mut out, zip32(central_start));
    u16_le(&mut out, 0);
    Ok(out)
}

#[cfg(test)]
mod tests;
