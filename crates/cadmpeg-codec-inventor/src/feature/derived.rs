// SPDX-License-Identifier: Apache-2.0
//! Stored derived-body source operands.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use super::{
    PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload,
};
use crate::layout::{
    pm_dc_content_header as header, pm_dc_derived_component_fields as fields,
    pm_dc_derived_component_suffix as suffix, pm_dc_derived_source_field as source_field,
    pm_dc_derived_source_stamp as stamp,
};
use crate::pmdc::{content_header, state_content_header, Cursor, PmDcReference};
use crate::sketch::PmDcTransform;

pub(crate) const CLASS_ID: super::ClassId =
    super::ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
        0xee, 0xcb, 0x29, 0x8e, 0x2c, 0x45, 0x65, 0x08, 0x20, 0x10, 0x4e, 0xb5, 0x5d, 0x06, 0x2c,
        0x80,
    ]));

const SOURCE_TYPE: [u8; 16] = [
    0x3c, 0xc0, 0x48, 0x21, 0x00, 0x42, 0x42, 0x0b, 0xa6, 0x21, 0x03, 0xb2, 0x0b, 0x7f, 0x48, 0x91,
];
const COMPONENT_TYPE: [u8; 16] = [
    0xa0, 0x34, 0x36, 0x3f, 0xd3, 0x11, 0x91, 0xb5, 0xc0, 0x00, 0x03, 0x85, 0xe8, 0x9c, 0x6b, 0x4f,
];
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Property {
    Source {
        component: PmDcReference,
    },
    Component {
        state: i32,
        references: [PmDcReference; 4],
        terminal: PmDcReference,
    },
}

impl Property {
    pub(crate) fn references(&self) -> Vec<PmDcReference> {
        match self {
            Self::Source { component } => vec![*component],
            Self::Component {
                references,
                terminal,
                ..
            } => references.iter().copied().chain([*terminal]).collect(),
        }
    }
}

pub(super) fn parser(
    type_id: [u8; 16],
    mut source: View<'_>,
    version: u8,
) -> Option<super::PropertyParser> {
    let (source_len, component_len) = record_lengths(version)?;
    match type_id {
        SOURCE_TYPE if source.remaining() == source_len => {
            source.take(source_len - source_field::LEN)?;
            local_reference(PmDcReference::from_packed(source.u32_le()?)).then_some(parse_source)
        }
        COMPONENT_TYPE if source.remaining() == component_len => {
            source.take(header::LEN)?;
            if source.i32_le()? != fields::STATE_VALUE {
                return None;
            }
            let references = [
                PmDcReference::from_packed(source.u32_le()?),
                PmDcReference::from_packed(source.u32_le()?),
                PmDcReference::from_packed(source.u32_le()?),
                PmDcReference::from_packed(source.u32_le()?),
            ];
            if !references.into_iter().all(local_reference)
                || source.u32_le()? != fields::MARKER_VALUE
                || source.u32_le()? != fields::COUNT_VALUE
            {
                return None;
            }
            if !local_reference(PmDcReference::from_packed(source.u32_le()?))
                || (version == 23 && source.u32_le()? != suffix::STATE_VALUE)
            {
                return None;
            }
            Some(parse_component)
        }
        _ => None,
    }
}

fn record_lengths(version: u8) -> Option<(usize, usize)> {
    match version {
        19 => Some((header::LEN + source_field::LEN, header::LEN + fields::LEN)),
        23 => Some((
            header::LEN + stamp::LEN + source_field::LEN,
            header::LEN + fields::LEN + suffix::LEN,
        )),
        _ => None,
    }
}

fn local_reference(reference: PmDcReference) -> bool {
    reference.qualified() && reference.index() != 0
}

fn parse_source(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    if record_lengths(version).is_none() {
        return Err(CodecError::malformed(
            "Inventor derived source version is outside the stored-body form",
        ));
    }
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let component = cursor.reference("derived component")?;
    cursor.finish("derived source")?;
    Ok(super::property(
        version,
        header,
        PmDcFeaturePropertyKind::Derived {
            definition: Property::Source { component },
        },
    ))
}

fn parse_component(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    if record_lengths(version).is_none() {
        return Err(CodecError::malformed(
            "Inventor derived component version is outside the stored-body form",
        ));
    }
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, version)?;
    let state = cursor.i32("derived component state")?;
    let references = [
        cursor.reference("derived component reference 0")?,
        cursor.reference("derived component reference 1")?,
        cursor.reference("derived transform")?,
        cursor.reference("derived component reference 3")?,
    ];
    if cursor.u32("derived component list marker")? != fields::MARKER_VALUE
        || cursor.u32("derived component list count")? != fields::COUNT_VALUE
    {
        return Err(CodecError::malformed(
            "Inventor derived component list is not empty",
        ));
    }
    let terminal = cursor.reference("derived component terminal")?;
    if version == 23 && cursor.u32("derived component suffix state")? != suffix::STATE_VALUE {
        return Err(CodecError::malformed(
            "Inventor derived component suffix state is unsupported",
        ));
    }
    cursor.finish("derived component")?;
    Ok(super::property(
        version,
        header,
        PmDcFeaturePropertyKind::Derived {
            definition: Property::Component {
                state,
                references,
                terminal,
            },
        },
    ))
}

pub(crate) struct SourceOperands<'a> {
    pub(crate) source: &'a PmDcFeatureProperty,
    pub(crate) transform: &'a PmDcTransform,
}

pub(crate) fn operands<'a>(
    feature: &PmDcFeature,
    mut property: impl FnMut(u32) -> Option<&'a PmDcFeatureProperty>,
    mut record: impl FnMut(u32) -> Option<(String, u64)>,
    transforms: &'a [PmDcTransform],
) -> Option<SourceOperands<'a>> {
    let (source_len, component_len) = record_lengths(feature.save_version_major)?;
    let slots = feature.properties.references();
    if feature.properties.marker() != 2
        || slots.len() != 3
        || slots.iter().any(|reference| !local_reference(*reference))
    {
        return None;
    }
    let source = property(slots[2].record_ordinal()?)?;
    let source_record = record(source.identity.record_ordinal)?;
    if source.save_version_major != feature.save_version_major
        || source.header.stamps.len() != usize::from(feature.save_version_major == 23)
        || source.header.stamps.iter().any(|stamp| *stamp < -1)
        || source.header.flags_extension.is_some()
        || source.identity.type_id.as_str() != crate::pmdc::type_id_string(SOURCE_TYPE)
        || source_record.0 != source.identity.type_id.as_str()
        || source_record.1 != cadmpeg_core::decode::u64_from_index(source_len)
    {
        return None;
    }
    let PmDcFeaturePropertyKind::Derived {
        definition: Property::Source { component },
    } = source.kind
    else {
        return None;
    };
    if !local_reference(component) {
        return None;
    }
    let source = property(component.record_ordinal()?)?;
    let source_record = record(source.identity.record_ordinal)?;
    if source.save_version_major != feature.save_version_major
        || !source.header.stamps.is_empty()
        || source.header.flags_extension.is_some()
        || source.identity.type_id.as_str() != crate::pmdc::type_id_string(COMPONENT_TYPE)
        || source_record.0 != source.identity.type_id.as_str()
        || source_record.1 != cadmpeg_core::decode::u64_from_index(component_len)
    {
        return None;
    }
    let PmDcFeaturePropertyKind::Derived {
        definition:
            Property::Component {
                state,
                references,
                terminal,
            },
    } = source.kind
    else {
        return None;
    };
    if state != fields::STATE_VALUE {
        return None;
    }
    let types = [
        "9f34363fd31191b5c0000385e89c6b4f",
        "604d8790d011f8d10008cabc0663dc09",
        "184d8790d011f8d10008cabc0663dc09",
        "d77e5d25d311f2b5c0000385e89c6b4f",
        "a7fbcb1db64f16cfe979019eae94a4df",
    ];
    for (reference, expected) in references.into_iter().chain([terminal]).zip(types) {
        if !local_reference(reference) || record(reference.record_ordinal()?)?.0 != expected {
            return None;
        }
    }
    let mut matches = transforms.iter().filter(|transform| {
        transform.identity.segment_token == feature.identity.segment_token
            && transform.save_version_major == feature.save_version_major
            && Some(transform.identity.record_ordinal) == references[2].record_ordinal()
    });
    let transform = matches.next()?;
    matches
        .next()
        .is_none()
        .then_some(SourceOperands { source, transform })
}

pub(crate) fn native_source_loss(
    labels: &[super::PmDcFeatureLabel],
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let count = labels
        .iter()
        .filter(|label| label.class_id() == CLASS_ID)
        .count();
    (count != 0).then(|| crate::loss::InventorLossCode::FeatureDerivedSourceNative.note(format!(
        "{count} derived component source construction(s), including external document selection and source placement, remain in native records. Stored result geometry supplies no replayable external construction."
    )))
}

#[cfg(test)]
mod tests;
