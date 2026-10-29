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
    pm_dc_derived_source_field as source_field,
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
const TAGGING_CLASS: super::ClassId =
    super::ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
        0xe8, 0x99, 0xdb, 0xdf, 0xd3, 0x11, 0x65, 0xaf, 0xc0, 0x00, 0x01, 0x85, 0xe8, 0x9c, 0x6b,
        0x4f,
    ]));

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
    if version != 19 {
        return None;
    }
    match type_id {
        SOURCE_TYPE if source.remaining() == header::LEN + source_field::LEN => {
            source.take(header::LEN)?;
            local_reference(PmDcReference::from_packed(source.u32_le()?)).then_some(parse_source)
        }
        COMPONENT_TYPE if source.remaining() == header::LEN + fields::LEN => {
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
            local_reference(PmDcReference::from_packed(source.u32_le()?)).then_some(parse_component)
        }
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
    if version != 19 {
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
    if version != 19 {
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
    let slots = feature.properties.references();
    if feature.save_version_major != 19
        || feature.properties.marker() != 2
        || slots.len() != 3
        || slots.iter().any(|reference| !local_reference(*reference))
    {
        return None;
    }
    let source = property(slots[2].record_ordinal()?)?;
    let source_record = record(source.identity.record_ordinal)?;
    if source.save_version_major != feature.save_version_major
        || source.identity.type_id.as_str() != crate::pmdc::type_id_string(SOURCE_TYPE)
        || source_record.0 != source.identity.type_id.as_str()
        || source_record.1 != cadmpeg_core::decode::u64_from_index(header::LEN + source_field::LEN)
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
        || source.identity.type_id.as_str() != crate::pmdc::type_id_string(COMPONENT_TYPE)
        || source_record.0 != source.identity.type_id.as_str()
        || source_record.1 != cadmpeg_core::decode::u64_from_index(header::LEN + fields::LEN)
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

pub(crate) fn native_tagging_loss(
    labels: &[super::PmDcFeatureLabel],
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    if !labels.iter().any(|label| label.class_id() == CLASS_ID) {
        return None;
    }
    let count = labels
        .iter()
        .filter(|label| label.class_id() == TAGGING_CLASS)
        .count();
    (count != 0).then(|| crate::loss::InventorLossCode::FeatureDerivedTaggingNative.note(format!(
        "{count} derived-part tagging operation(s) retain native result selections without a neutral construction rule."
    )))
}

#[cfg(test)]
mod tests;
