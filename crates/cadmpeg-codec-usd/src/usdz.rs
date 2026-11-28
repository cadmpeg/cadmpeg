// SPDX-License-Identifier: Apache-2.0
//! USDZ packages (spec §1): a ZIP archive whose first member is the root
//! layer.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

pub(crate) mod source;

/// The ZIP local file header signature.
const LOCAL_HEADER_MAGIC: &[u8; 4] = b"PK\x03\x04";
/// The file name extensions of a root layer.
const LAYER_EXTENSIONS: [&str; 3] = [".usda", ".usdc", ".usd"];
/// The alignment of member data in a package.
const DATA_ALIGNMENT: usize = 64;
/// The member name of the root layer this writer packages.
pub(crate) const ROOT_MEMBER: &str = "root.usda";
/// The most bytes of a written member: a package has no ZIP64 records.
// resource-cap: MAX_MEMBER = N
pub(crate) const MAX_MEMBER: u64 = 0xFFFF_FFFF;

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

/// The ZIP version that stored members need.
const VERSION_NEEDED: u16 = 10;
/// The fixed part of a local file header.
const LOCAL_HEADER_LEN: usize = 30;
/// The fixed part of a central directory header.
const CENTRAL_HEADER_LEN: u32 = 46;
/// The header of a padding extra field: its id and its size.
const PADDING_HEADER_LEN: u16 = 4;
/// The extra field id of the padding.
const PADDING_ID: u16 = 0x1986;

/// The fields of one member that the archive can address.
struct Admitted {
    name_len: u16,
    padding_len: u16,
    size: u32,
    /// The archive end after the member's data.
    end: u32,
    /// The central directory length with the member's header.
    central_len: u32,
    /// The member count with the member.
    count: u16,
}

/// The fields of a member of `name` and `data_len` bytes written at `end`,
/// after `count` members whose central headers take `central_len` bytes, or
/// `None` where an archive without ZIP64 records cannot address it: a name
/// past 16 bits, a size or the archive with its central directory past 32
/// bits, or a count that reaches `u16::MAX`.
fn admit(name: &str, data_len: usize, end: u32, central_len: u32, count: u16) -> Option<Admitted> {
    let name_len = u16::try_from(name.len()).ok()?;
    let unpadded = usize::try_from(end)
        .ok()?
        .checked_add(LOCAL_HEADER_LEN + usize::from(name_len))?;
    let mut padding = (DATA_ALIGNMENT - unpadded % DATA_ALIGNMENT) % DATA_ALIGNMENT;
    if padding > 0 && padding < usize::from(PADDING_HEADER_LEN) {
        padding += DATA_ALIGNMENT;
    }
    let end = u32::try_from(unpadded.checked_add(padding)?.checked_add(data_len)?).ok()?;
    let central_len = central_len
        .checked_add(CENTRAL_HEADER_LEN)?
        .checked_add(u32::from(name_len))?;
    end.checked_add(central_len)?;
    Some(Admitted {
        name_len,
        padding_len: u16::try_from(padding).ok()?,
        size: u32::try_from(data_len).ok()?,
        end,
        central_len,
        // A count of `u16::MAX` marks a ZIP64 archive.
        count: count.checked_add(1).filter(|count| *count < u16::MAX)?,
    })
}

/// The layer the package holds in place of a root layer it cannot address.
const EMPTY_LAYER: &[u8] = b"#usda 1.0\n";

/// The `.usdz` package of the written root layer `root` and the texture
/// `members` its asset paths name, with the losses of the packaging.
///
/// The root layer is the first member. One past the archive limits
/// ([`admit`]) leaves the package with the empty layer as its root, charged
/// `export:output.empty`. A texture member past them is left out with the
/// asset paths that name it, and each attribute takes its fallback value.
pub(crate) fn package_layer(
    root: &[u8],
    members: &[(String, Vec<u8>)],
) -> (Vec<u8>, Vec<LossNote>) {
    package_layer_by(root, members, admit)
}

/// [`package_layer`] with the archive limits of `admit`.
fn package_layer_by(
    root: &[u8],
    members: &[(String, Vec<u8>)],
    admit: impl Fn(&str, usize, u32, u32, u16) -> Option<Admitted>,
) -> (Vec<u8>, Vec<LossNote>) {
    let all: Vec<(&str, &[u8])> = std::iter::once((ROOT_MEMBER, root))
        .chain(
            members
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
        )
        .collect();
    let (bytes, left_out) = package_by(&all, &admit);
    if left_out.iter().any(|name| name == ROOT_MEMBER) {
        let (bytes, rest) = package_by(&[(ROOT_MEMBER, EMPTY_LAYER)], &admit);
        debug_assert!(rest.is_empty(), "the archive addresses the empty layer");
        let note = cadmpeg_ir::codec::write::output_empty(format!(
            "the root layer of {} bytes is past the 32-bit sizes of a ZIP archive without \
             ZIP64 records; the package holds an empty root layer",
            root.len()
        ));
        return (bytes, vec![note]);
    }
    if left_out.is_empty() {
        return (bytes, Vec::new());
    }
    // The root layer only shrinks, so each member admitted after the larger
    // root layer is admitted after the smaller one.
    let root = std::str::from_utf8(root).map_or_else(
        |_| root.to_vec(),
        |text| crate::writer::text::without_assets(text, &left_out).into_bytes(),
    );
    let kept: Vec<(&str, &[u8])> = std::iter::once((ROOT_MEMBER, root.as_slice()))
        .chain(
            members
                .iter()
                .filter(|(name, _)| !left_out.contains(name))
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
        )
        .collect();
    let (bytes, again) = package_by(&kept, &admit);
    debug_assert!(again.is_empty(), "a smaller root layer admits the members");
    let notes = cadmpeg_ir::codec::write::value_unwritable(
        "USDZ",
        "leaves out the texture members that a ZIP archive without ZIP64 records cannot \
         address, and the asset paths that name them; each attribute takes its fallback value",
        left_out,
    );
    (bytes, notes.into_iter().collect())
}

/// A package of the stored `members`, by name and content, in order, each
/// with its data at a multiple of 64 bytes, and the names of the members
/// left out.
///
/// The archive has no ZIP64 records ([`admit`]). A member past its limits
/// is left out, and the members after it are written where they fit.
#[cfg(test)]
pub(crate) fn package(members: &[(&str, &[u8])]) -> (Vec<u8>, Vec<String>) {
    package_by(members, admit)
}

/// [`package`] with the archive limits of `admit`.
fn package_by(
    members: &[(&str, &[u8])],
    admit: impl Fn(&str, usize, u32, u32, u16) -> Option<Admitted>,
) -> (Vec<u8>, Vec<String>) {
    let mut out = Vec::new();
    let mut entries = Vec::new();
    // The archive end, the central directory length and the member count
    // of the members written so far.
    let (mut end, mut central_len, mut count) = (0_u32, 0_u32, 0_u16);
    let mut left_out = Vec::new();
    for (name, data) in members {
        let Some(member) = admit(name, data.len(), end, central_len, count) else {
            left_out.push((*name).to_owned());
            continue;
        };
        let crc = crc32fast::hash(data);
        out.extend_from_slice(LOCAL_HEADER_MAGIC);
        u16_le(&mut out, VERSION_NEEDED);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u32_le(&mut out, crc);
        u32_le(&mut out, member.size);
        u32_le(&mut out, member.size);
        u16_le(&mut out, member.name_len);
        u16_le(&mut out, member.padding_len);
        out.extend_from_slice(name.as_bytes());
        if member.padding_len > 0 {
            // A padding is at least its own header (`admit`).
            let filler = member.padding_len - PADDING_HEADER_LEN;
            u16_le(&mut out, PADDING_ID);
            u16_le(&mut out, filler);
            out.resize(out.len() + usize::from(filler), 0);
        }
        out.extend_from_slice(data);
        debug_assert_eq!(u32::try_from(out.len()).ok(), Some(member.end));
        entries.push((name, member.name_len, member.size, crc, end));
        (end, central_len, count) = (member.end, member.central_len, member.count);
    }
    let central_start = end;
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
    out.extend_from_slice(b"PK\x05\x06");
    u16_le(&mut out, 0);
    u16_le(&mut out, 0);
    u16_le(&mut out, count);
    u16_le(&mut out, count);
    u32_le(&mut out, central_len);
    u32_le(&mut out, central_start);
    u16_le(&mut out, 0);
    (out, left_out)
}

#[cfg(test)]
mod tests;
