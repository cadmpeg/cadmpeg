// SPDX-License-Identifier: Apache-2.0
//! Single-entry Mirror instance maps.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::feature::{property, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload};
use crate::pmdc::{content_header, Cursor, PmDcReference};

pub(crate) const TYPE: [u8; 16] = [
    0x54, 0xff, 0x26, 0x1a, 0x4a, 0x42, 0x57, 0x67, 0x64, 0x92, 0x95, 0xab, 0xae, 0x7c, 0x3d, 0xb8,
];

pub(crate) fn parse(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<Option<PmDcFeaturePropertyPayload>, CodecError> {
    use crate::layout::pm_dc_mirror_instance_map_single_tail_v19 as layout;

    if version != 19 {
        return Ok(None);
    }
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    if cursor.remaining() != layout::LEN {
        return Ok(None);
    }
    let mut tail = cursor.into_view();
    if tail.req_i32_le()? != layout::STATE_VALUE
        || tail.req_u16_le()? != layout::MAP_MARKER_VALUE
        || tail.req_u16_le()? != layout::MAP_SIGNATURE_VALUE
        || tail.req_u32_le()? != layout::MAP_COUNT_VALUE
        || tail.req_u32_le()? != layout::MAP_VALUE_VALUE
        || tail.req_u32_le()? != layout::SOURCE_TAG_VALUE
    {
        return Ok(None);
    }
    let source = PmDcReference::from_packed(tail.req_u32_le()?);
    if tail.req_u16_le()? != layout::RESULT_MARKER_VALUE
        || tail.req_u16_le()? != layout::RESULT_SIGNATURE_VALUE
        || tail.req_u32_le()? != layout::RESULT_COUNT_VALUE
        || tail.req_u32_le()? != layout::RESULT_METADATA0_VALUE
        || tail.req_u32_le()? != layout::RESULT_METADATA1_VALUE
    {
        return Ok(None);
    }
    let result = PmDcReference::from_packed(tail.req_u32_le()?);
    if tail.req_u32_le()? != layout::TERMINAL_VALUE_VALUE
        || tail.req_u32_le()? != layout::OWNER_COUNT_VALUE
    {
        return Ok(None);
    }
    let owner = PmDcReference::from_packed(tail.req_u32_le()?);
    if !reference_form(source, result, owner) {
        return Ok(None);
    }
    Ok(Some(property(
        version,
        header,
        PmDcFeaturePropertyKind::Mirror {
            definition: super::Property::InstanceMap {
                source,
                result,
                owner,
            },
        },
    )))
}

pub(super) fn reference_form(
    source: PmDcReference,
    result: PmDcReference,
    owner: PmDcReference,
) -> bool {
    [source, result, owner]
        .into_iter()
        .all(|reference| reference.qualified() && reference.index() != 0)
        && source != result
        && source != owner
        && result != owner
}

#[cfg(test)]
mod tests;
