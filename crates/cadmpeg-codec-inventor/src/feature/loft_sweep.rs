// SPDX-License-Identifier: Apache-2.0
//! Loft section references and sweep controls.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{
    property, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload,
    PmDcFeatureReferenceFamily, PmDcKernelEntityLink,
};
use crate::pmdc::{
    content_header, type_id_string, Cursor, PmDcListMetadata, PmDcReference, PmDcReferenceList,
};

pub(crate) mod controls;
pub(super) mod projection;
pub(crate) mod slots;
pub(crate) mod validation;

pub(crate) fn retained_control_losses<'a>(
    definitions: impl IntoIterator<Item = &'a cadmpeg_ir::features::FeatureDefinition>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};
    use slots::Family;

    let mut sweeps = 0_usize;
    let mut lofts = 0_usize;
    for definition in definitions {
        if let FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Other(kind),
            ..
        }) = definition
        {
            if kind == Family::Sweep.native_name() {
                sweeps += 1;
            } else if kind == Family::SolidLoft.native_name()
                || kind == Family::SurfaceLoft.native_name()
            {
                lofts += 1;
            }
        }
    }
    let mut losses = Vec::new();
    if sweeps != 0 {
        losses.push(crate::loss::InventorLossCode::FeatureSweepControlsUnresolved.note(format!(
            "Retained {sweeps} native Inventor sweep feature(s): the operands, controls, or complete per-feature result topology do not close the neutral sweep construction."
        )));
    }
    if lofts != 0 {
        losses.push(crate::loss::InventorLossCode::FeatureLoftControlsUnresolved.note(format!(
            "Retained {lofts} native Inventor loft feature(s): the section conditions, operands, controls, or complete per-feature result topology do not close the neutral loft construction."
        )));
    }
    losses
}

pub(super) const SECTION_LIST_TYPE: [u8; 16] = [
    0x50, 0x9e, 0x2f, 0x31, 0xd1, 0x11, 0x03, 0x44, 0x00, 0x08, 0x81, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const SECTION_TYPE: [u8; 16] = [
    0xe8, 0x50, 0x31, 0x4b, 0xd1, 0x11, 0xd6, 0x41, 0x00, 0x08, 0x80, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const SECTION_CONDITION_TYPE: [u8; 16] = [
    0x6c, 0x1c, 0x6c, 0x3c, 0x9a, 0x43, 0xe3, 0x94, 0x73, 0x18, 0xe2, 0xbb, 0xd0, 0xc3, 0x9f, 0xf8,
];
pub(super) const LOFT_MODE_TYPE: [u8; 16] = [
    0x02, 0xe2, 0xe3, 0xe0, 0xd1, 0x11, 0x48, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const SWEEP_MODE_TYPE: [u8; 16] = [
    0xb7, 0x84, 0x9c, 0xa2, 0x6d, 0x48, 0x11, 0xe3, 0x8b, 0x95, 0xb3, 0xa0, 0x60, 0x51, 0xb4, 0x40,
];
pub(super) const SWEEP_ORIENTATION_TYPE: [u8; 16] = [
    0xed, 0x2e, 0xf0, 0x31, 0x90, 0x4b, 0x9b, 0x04, 0xd7, 0x99, 0x18, 0xb9, 0x1f, 0x16, 0x19, 0xc8,
];
pub(super) const SWEEP_SCALING_TYPE: [u8; 16] = [
    0xe2, 0x11, 0xb0, 0x5c, 0x3e, 0x46, 0x58, 0x3e, 0x92, 0x8a, 0x0c, 0xbf, 0x3a, 0x08, 0x58, 0x34,
];

pub(super) fn parse_sections(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    if cursor.u32("loft section list marker")? != 0x3000_0002 {
        return Err(CodecError::malformed(
            "Inventor loft section list marker is invalid",
        ));
    }
    let count = cursor.u32("loft section list count")?;
    let metadata = if count == 0 {
        None
    } else {
        Some(PmDcListMetadata::U32(
            cursor.u32_array("loft section list metadata")?,
        ))
    };
    let mut source = cursor.into_view();
    let count = source.counted(u64::from(count), 4).ok_or_else(|| {
        CodecError::malformed("Inventor loft section list count exceeds remaining payload")
    })?;
    let mut items = ctx.alloc_filled(
        count.get(),
        PmDcReference::from_packed(0),
        "retain Inventor loft section references",
    )?;
    for reference in &mut items {
        *reference = crate::reader::pmdc_reference(&mut source, "loft section reference")?;
    }
    Cursor::new(source).finish("loft section list")?;
    let items = PmDcReferenceList::new(2, metadata, items).ok_or_else(|| {
        CodecError::malformed("Inventor loft section list metadata disagrees with length")
    })?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::LoftSections,
            items,
        },
    ))
}

/// Admits section records with an empty type-7 map.
pub(super) fn parse_section(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let entity_link = cursor.reference("loft section entity link")?;
    let value = cursor.u8("loft section value")?;
    let condition = cursor.reference("loft section condition")?;
    let parameters = [
        cursor.reference("loft section first parameter")?,
        cursor.reference("loft section second parameter")?,
    ];
    let map_value = cursor.u32("loft section map value")?;
    let marker = cursor.u32("loft section map marker")?;
    let count = cursor.u32("loft section map count")?;
    if map_value != 0 || marker != 0x3000_0007 || count != 0 {
        return Err(CodecError::malformed(
            "Inventor loft section requires an empty type-7 map",
        ));
    }
    let source_value = cursor.u32("loft section source value")?;
    let option = cursor.reference("loft section option")?;
    cursor.finish("loft section")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::LoftSection {
            entity_link,
            value,
            condition,
            parameters,
            map_value,
            source_value,
            option,
        },
    ))
}

pub(crate) fn valid_references<'a, 'b>(
    property: &PmDcFeatureProperty,
    type_at: impl Fn(u32) -> Option<&'a str>,
    link_at: impl Fn(u32) -> Option<&'b PmDcKernelEntityLink>,
) -> bool {
    let type_id = property.identity.type_id.as_str();
    let kind_matches_type = if type_id == type_id_string(SECTION_TYPE) {
        matches!(property.kind, PmDcFeaturePropertyKind::LoftSection { .. })
    } else if type_id == type_id_string(SECTION_LIST_TYPE) {
        matches!(
            property.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::LoftSections,
                ..
            }
        )
    } else if [
        SECTION_CONDITION_TYPE,
        LOFT_MODE_TYPE,
        SWEEP_MODE_TYPE,
        SWEEP_ORIENTATION_TYPE,
        SWEEP_SCALING_TYPE,
    ]
    .into_iter()
    .any(|control| type_id == type_id_string(control))
    {
        matches!(
            property.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: super::PmDcFeatureEnumFamily::Auxiliary,
                ..
            }
        )
    } else {
        true
    };
    let entity_link_type = type_id_string(super::KERNEL_ENTITY_LINK_TYPE);
    let body_type = type_id_string(crate::kernel::KERNEL_RECORD_TYPE_ID);
    let condition_type = type_id_string(SECTION_CONDITION_TYPE);
    let parameter_type = type_id_string(crate::design::PARAMETER_FULL_TYPE);
    let boolean_type = type_id_string(super::BOOLEAN_TYPE);
    let section_type = type_id_string(SECTION_TYPE);
    if !kind_matches_type {
        return false;
    }
    let referenced_type = |reference: PmDcReference| reference.record_ordinal().and_then(&type_at);
    match &property.kind {
        PmDcFeaturePropertyKind::LoftSection {
            entity_link,
            condition,
            parameters,
            map_value,
            option,
            ..
        } => {
            *map_value == 0
                && property.identity.type_id.as_str() == type_id_string(SECTION_TYPE)
                && referenced_type(*entity_link) == Some(entity_link_type.as_str())
                && entity_link
                    .record_ordinal()
                    .and_then(link_at)
                    .is_some_and(|link| {
                        link.identity.segment_token == property.identity.segment_token
                            && Some(link.identity.record_ordinal) == entity_link.record_ordinal()
                            && link.identity.type_id.as_str() == entity_link_type
                            && link.selection_source == property.header.source_index
                            && referenced_type(link.body) == Some(body_type.as_str())
                    })
                && referenced_type(*condition) == Some(condition_type.as_str())
                && parameters
                    .iter()
                    .all(|reference| referenced_type(*reference) == Some(parameter_type.as_str()))
                && (option.index() == 0 || referenced_type(*option) == Some(boolean_type.as_str()))
        }
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::LoftSections,
            items,
        } => {
            property.identity.type_id.as_str() == type_id_string(SECTION_LIST_TYPE)
                && items
                    .references()
                    .iter()
                    .all(|reference| referenced_type(*reference) == Some(section_type.as_str()))
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests;
