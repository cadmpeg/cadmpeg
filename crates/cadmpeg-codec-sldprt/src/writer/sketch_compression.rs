// SPDX-License-Identifier: Apache-2.0
//! Optional source compression layout for freshly authored sketch streams.

use std::collections::BTreeSet;
use std::io::Write;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

pub(super) fn from_fidelity(fidelity: Option<&cadmpeg_ir::SourceFidelity>) -> BTreeSet<String> {
    let Some(bytes) = fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .and_then(cadmpeg_ir::RetainedSourceRecord::data)
    else {
        return BTreeSet::new();
    };
    let arena = DecodeArena::new();
    let Ok((ctx, view)) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::desktop())
    else {
        return BTreeSet::new();
    };
    // This is optional layout evidence. An unavailable or invalid source must
    // never prevent authoring the current typed document without that hint.
    let Ok(scan) = crate::container::scan(&ctx, view) else {
        return BTreeSet::new();
    };
    from_scan(Some(&scan))
}

pub(super) fn from_scan(scan: Option<&crate::container::ContainerScan<'_>>) -> BTreeSet<String> {
    let mut configurations = BTreeSet::new();
    let Some(scan) = scan else {
        return configurations;
    };
    for section in scan.sections() {
        let Some(name) = section
            .name()
            .filter(|name| name.ends_with("ResolvedFeatures"))
        else {
            continue;
        };
        // Extracted locations have passed header and decompression checks.
        // Direct PS streams need no nested compression; wrapper/member locations do.
        if section.ps_streams().iter().any(|stream| {
            section
                .payload()
                .get(stream.offset..)
                .is_some_and(|bytes| !bytes.starts_with(b"PS\0\0"))
        }) {
            configurations.insert(
                crate::container::configuration_index(name)
                    .map_or_else(|| "0".to_owned(), |index| index.to_string()),
            );
        }
    }
    configurations
}

/// One standard chained frame over exactly the newly generated PS stream.
/// No old wrapper, compressed member, body bytes, or padding are copied.
pub(crate) fn wrap(stream: &[u8]) -> Result<Vec<u8>, CodecError> {
    use crate::layout::{parasolid_chain_frame_header as frame, zlb_wrapper_header as wrapper};
    let uncompressed = u32::try_from(stream.len())
        .map_err(|_| CodecError::NotImplemented("SLDPRT sketch stream exceeds u32".into()))?;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(stream)?;
    let member = encoder.finish()?;
    let chain_len = wrapper::MAGIC_VALUE
        .len()
        .checked_add(frame::LEN)
        .and_then(|len| len.checked_add(member.len()))
        .and_then(|len| u32::try_from(len).ok())
        .ok_or_else(|| {
            CodecError::NotImplemented("SLDPRT sketch compression frame exceeds u32".into())
        })?;
    let member_len = u32::try_from(member.len())
        .map_err(|_| CodecError::NotImplemented("SLDPRT sketch zlib member exceeds u32".into()))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&chain_len.to_le_bytes());
    bytes.extend_from_slice(&wrapper::MAGIC_VALUE);
    bytes.extend_from_slice(&uncompressed.to_le_bytes());
    bytes.extend_from_slice(&member_len.to_le_bytes());
    bytes.extend_from_slice(&member);
    Ok(bytes)
}

#[cfg(test)]
mod tests;
