// SPDX-License-Identifier: Apache-2.0
//! USDZ packages (spec §1): a ZIP archive whose first member is the root
//! layer.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

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

fn too_large() -> CodecError {
    CodecError::NotImplemented("a USDZ member above 4 GiB needs ZIP64".to_owned())
}

/// A package of the stored `members`, by name and content, in order, each
/// with its data at a multiple of 64 bytes.
pub(crate) fn package(members: &[(&str, &[u8])]) -> Result<Vec<u8>, CodecError> {
    /// The ZIP version that stored members need.
    const VERSION_NEEDED: u16 = 10;
    /// The fixed part of a local file header.
    const LOCAL_HEADER_LEN: usize = 30;
    /// The header of a padding extra field: its id and its size.
    const PADDING_HEADER_LEN: usize = 4;
    /// The extra field id of the padding.
    const PADDING_ID: u16 = 0x1986;
    let mut out = Vec::new();
    let mut entries = Vec::new();
    for (name, data) in members {
        let name_len = u16::try_from(name.len()).map_err(|_| too_large())?;
        let size = u32::try_from(data.len()).map_err(|_| too_large())?;
        let crc = crc32fast::hash(data);
        let offset = u32::try_from(out.len()).map_err(|_| too_large())?;
        let unpadded = out.len() + LOCAL_HEADER_LEN + name.len();
        let mut padding = (DATA_ALIGNMENT - unpadded % DATA_ALIGNMENT) % DATA_ALIGNMENT;
        if padding > 0 && padding < PADDING_HEADER_LEN {
            padding += DATA_ALIGNMENT;
        }
        let padding_len = u16::try_from(padding).map_err(|_| too_large())?;
        out.extend_from_slice(LOCAL_HEADER_MAGIC);
        u16_le(&mut out, VERSION_NEEDED);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, crc);
        u32_le(&mut out, size);
        u32_le(&mut out, size);
        u16_le(&mut out, name_len);
        u16_le(&mut out, padding_len);
        out.extend_from_slice(name.as_bytes());
        if padding > 0 {
            u16_le(&mut out, PADDING_ID);
            u16_le(
                &mut out,
                u16::try_from(padding - PADDING_HEADER_LEN).map_err(|_| too_large())?,
            );
            out.resize(out.len() + padding - PADDING_HEADER_LEN, 0);
        }
        out.extend_from_slice(data);
        entries.push((name, name_len, size, crc, offset));
    }
    let count = u16::try_from(entries.len()).map_err(|_| too_large())?;
    let central_start = u32::try_from(out.len()).map_err(|_| too_large())?;
    for (name, name_len, size, crc, offset) in entries {
        out.extend_from_slice(b"PK\x01\x02");
        u16_le(&mut out, VERSION_NEEDED);
        u16_le(&mut out, VERSION_NEEDED);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, crc);
        u32_le(&mut out, size);
        u32_le(&mut out, size);
        u16_le(&mut out, name_len);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, 0);
        u32_le(&mut out, offset);
        out.extend_from_slice(name.as_bytes());
    }
    let central_size = u32::try_from(out.len())
        .map_err(|_| too_large())?
        .checked_sub(central_start)
        .ok_or_else(too_large)?;
    out.extend_from_slice(b"PK\x05\x06");
    u16_le(&mut out, 0);
    u16_le(&mut out, 0);
    u16_le(&mut out, count);
    u16_le(&mut out, count);
    u32_le(&mut out, central_size);
    u32_le(&mut out, central_start);
    u16_le(&mut out, 0);
    Ok(out)
}

#[cfg(test)]
mod tests;
