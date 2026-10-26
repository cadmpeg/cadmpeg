// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal style operand prefixes.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::layout::pm_dc_sheet_metal_style_operand_prefix as wide;
use crate::layout::pm_dc_sheet_metal_style_operand_prefix_legacy as legacy;
use crate::pmdc::{Cursor, PmDcReference};
use crate::record_identity::{Located, RecordPayload};

pub(crate) const TYPE: [u8; 16] = [
    0x13, 0x23, 0x45, 0x60, 0xd3, 0x11, 0x7a, 0x0d, 0x60, 0x00, 0xec, 0xb2, 0x1d, 0x6e, 0xef, 0xb0,
];

pub(crate) type Style = Located<StylePayload>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StylePayload {
    pub(crate) save_version_major: u8,
    prefix: NativeBytes<Vec<u8>>,
    pub(crate) thickness: PmDcReference,
    pub(crate) bend_radius: PmDcReference,
    pub(crate) bend_relief_form: PmDcReference,
    pub(crate) bend_relief_width: PmDcReference,
    pub(crate) bend_relief_depth: PmDcReference,
    pub(crate) corner_relief_form: PmDcReference,
    pub(crate) corner_relief_size: PmDcReference,
    pub(crate) seam_gap: PmDcReference,
    pub(crate) minimum_remnant: PmDcReference,
    pub(crate) option: PmDcReference,
    values: [u32; 3],
    pub(crate) transition_form: PmDcReference,
    pub(crate) transition_radius: PmDcReference,
    suffix: NativeBytes<Vec<u8>>,
}

impl RecordPayload for StylePayload {
    const KIND: &'static str = "sheet-metal-style";
}

pub(crate) fn prefix_len(version: u8) -> Option<usize> {
    match version {
        20 | 23 => Some(legacy::THICKNESS),
        29 | 31 => Some(wide::THICKNESS),
        _ => None,
    }
}

fn retain(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<NativeBytes<Vec<u8>>, CodecError> {
    ctx.retain_bytes(
        u64_from_index(bytes.len()),
        "retain Inventor sheet-metal style fields",
    )?;
    let mut copy = ctx.alloc_filled(bytes.len(), 0, "copy Inventor sheet-metal style fields")?;
    copy.copy_from_slice(bytes);
    Ok(NativeBytes::new(copy))
}

pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    mut source: View<'_>,
    version: u8,
) -> Result<StylePayload, CodecError> {
    let length = prefix_len(version)
        .ok_or_else(|| CodecError::malformed("unsupported Inventor sheet-metal style prefix"))?;
    let prefix = retain(
        ctx,
        crate::reader::take(&mut source, length, "sheet-metal style prefix")?,
    )?;
    let mut cursor = Cursor::new(source);
    let thickness = cursor.reference("style thickness")?;
    let bend_radius = cursor.reference("style bend radius")?;
    let bend_relief_form = cursor.reference("style bend relief form")?;
    let bend_relief_width = cursor.reference("style bend relief width")?;
    let bend_relief_depth = cursor.reference("style bend relief depth")?;
    let corner_relief_form = cursor.reference("style corner relief form")?;
    let corner_relief_size = cursor.reference("style corner relief size")?;
    let seam_gap = cursor.reference("style seam gap")?;
    let minimum_remnant = cursor.reference("style minimum remnant")?;
    let option = cursor.reference("style option")?;
    let values = cursor.u32_array::<3>("style values")?;
    let transition_form = cursor.reference("style transition form")?;
    let transition_radius = cursor.reference("style transition radius")?;
    let mut tail = cursor.into_view();
    let remaining = tail.remaining();
    let suffix = retain(
        ctx,
        crate::reader::take(&mut tail, remaining, "style suffix")?,
    )?;
    Ok(StylePayload {
        save_version_major: version,
        prefix,
        thickness,
        bend_radius,
        bend_relief_form,
        bend_relief_width,
        bend_relief_depth,
        corner_relief_form,
        corner_relief_size,
        seam_gap,
        minimum_remnant,
        option,
        values,
        transition_form,
        transition_radius,
        suffix,
    })
}

pub(crate) fn valid<'a>(style: &Style, type_at: impl Fn(u32) -> Option<&'a str>) -> bool {
    let parameter = super::PARAMETER_TYPE;
    let references = [
        (style.thickness, parameter),
        (style.bend_radius, parameter),
        (style.bend_relief_form, super::RELIEF_FORM_TYPE),
        (style.bend_relief_width, parameter),
        (style.bend_relief_depth, parameter),
        (style.corner_relief_form, super::RELIEF_FORM_TYPE),
        (style.corner_relief_size, parameter),
        (style.seam_gap, parameter),
        (style.minimum_remnant, parameter),
        (style.option, super::super::BOOLEAN_TYPE),
        (style.transition_form, super::TRANSITION_FORM_TYPE),
        (style.transition_radius, parameter),
    ];
    style.identity.type_id.as_str() == crate::pmdc::type_id_string(TYPE)
        && type_at(style.identity.record_ordinal)
            .is_some_and(|actual| actual == crate::pmdc::type_id_string(TYPE))
        && prefix_len(style.save_version_major) == Some(style.prefix.as_ref().len())
        && references.into_iter().all(|(reference, expected)| {
            reference.record_ordinal().is_none_or(|ordinal| {
                type_at(ordinal)
                    .is_some_and(|actual| actual == crate::pmdc::type_id_string(expected))
            })
        })
}

#[cfg(test)]
mod tests;
