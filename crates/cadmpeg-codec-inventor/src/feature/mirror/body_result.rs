// SPDX-License-Identifier: Apache-2.0
//! Body selections of generated Mirror nodes.

use cadmpeg_ir::native::bytes::NativeBytes;

use super::{MirrorIndex, Property};
use crate::feature::{
    ClassId, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, PmDcPatternFeature, FEATURE_TYPE,
};
use crate::layout::pm_dc_mirror_generated_body_feature_v19 as layout;
use crate::pmdc::{PmDcListMetadata, PmDcReference};
use crate::record_identity::RecordTypeId;

pub(crate) const CLASS: ClassId = ClassId(NativeBytes::new([
    0x5b, 0xb8, 0x7e, 0x87, 0xac, 0x4a, 0x0f, 0x14, 0xd3, 0x14, 0xef, 0x89, 0xad, 0x8b, 0x5a, 0x7f,
]));
const GENERATED_MODE: u32 = 2;
const REPLACEMENT_FLAGS: u32 = 0x6200;
const SOURCE_BODY_FLAGS: u32 = 0x0300_4200;

fn one<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn version19_property(property: &PmDcFeatureProperty) -> bool {
    property.save_version_major == 19
        && property.header.flags_extension.is_none()
        && property.header.stamps.is_empty()
}

fn external_body(property: &PmDcFeatureProperty) -> Option<PmDcReference> {
    match property.kind {
        PmDcFeaturePropertyKind::SurfaceBody { body } if !body.qualified() && body.index() != 0 => {
            Some(body)
        }
        _ => None,
    }
}

pub(super) fn replacement<'a>(
    index: &MirrorIndex<'a>,
    mirror: &PmDcPatternFeature,
    label: &PmDcFeatureLabel,
) -> Option<&'a PmDcFeatureProperty> {
    let token = mirror.identity.segment_token.as_str();
    if mirror.save_version_major != 19
        || mirror.header.flags != REPLACEMENT_FLAGS
        || mirror.header.flags_extension.is_some()
        || !mirror.header.stamps.is_empty()
        || !mirror.participants.references().is_empty()
        || !label.participants.references().is_empty()
        || !label.header.owner.qualified()
        || mirror.property_slots.get(8)?.index() != 0
        || mirror.property_slots.get(9)?.index() != 0
        || mirror
            .property_slots
            .iter()
            .any(|reference| reference.qualified() != (reference.index() != 0))
        || !matches!(
            index.property(token, *mirror.property_slots.get(11)?)?.kind,
            PmDcFeaturePropertyKind::Boolean {
                name_value: 0,
                value: false,
                ..
            }
        )
    {
        return None;
    }
    let collection = index.property(token, *mirror.property_slots.get(10)?)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    let [selected] = items.references() else {
        return None;
    };
    if !version19_property(collection)
        || collection.header.flags != 0
        || items.marker() != 2
        || items.metadata() != Some(&PmDcListMetadata::U32([1, 0]))
        || !selected.qualified()
    {
        return None;
    }
    let body = index.property(token, *selected)?;
    if !version19_property(body) || body.header.flags != SOURCE_BODY_FLAGS {
        return None;
    }
    external_body(body)?;
    Some(body)
}

pub(super) fn resolve<'a>(
    index: &MirrorIndex<'a>,
    mirror: &PmDcPatternFeature,
    label: &PmDcFeatureLabel,
    map: &PmDcFeatureProperty,
    collection: &PmDcFeatureProperty,
) -> Option<&'a PmDcFeatureProperty> {
    if mirror.save_version_major != 19
        || !version19_property(map)
        || !version19_property(collection)
        || mirror.header.flags_extension.is_some()
        || !mirror.header.stamps.is_empty()
        || label.participants.metadata() != Some(&PmDcListMetadata::U32([1, 0]))
        || !label.header.owner.qualified()
        || mirror
            .property_slots
            .iter()
            .any(|reference| reference.qualified() != (reference.index() != 0))
    {
        return None;
    }
    let PmDcFeaturePropertyKind::Mirror {
        definition:
            Property::InstanceMap {
                source,
                result,
                owner,
            },
    } = &map.kind
    else {
        return None;
    };
    let [generated] = label.participants.references() else {
        return None;
    };
    if *owner != PmDcReference::new(mirror.identity.record_ordinal.checked_add(1)?, true)?
        || result != generated
    {
        return None;
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    let [selected] = items.references() else {
        return None;
    };
    if selected != source
        || !selected.qualified()
        || items.metadata() != Some(&PmDcListMetadata::U32([1, 0]))
    {
        return None;
    }
    let token = mirror.identity.segment_token.as_str();
    let source_body = index.property(token, *source)?;
    let source_reference = external_body(source_body)?;
    if !version19_property(source_body)
        || source_body.header.source_index == 0
        || mirror.header.source_index == 0
    {
        return None;
    }
    let (features, labels) = index.generated_nodes?;
    let node = one(features.iter().filter(|node| {
        node.identity.segment_token.as_str() == token
            && Some(node.identity.record_ordinal) == result.record_ordinal()
    }))?;
    let node_label = one(labels.iter().filter(|label| {
        label.identity.segment_token.as_str() == token
            && label.header.owner.record_ordinal() == Some(node.identity.record_ordinal)
    }))?;
    if node.identity.type_id != RecordTypeId::from_bytes(FEATURE_TYPE)
        || node.save_version_major != 19
        || node.header.flags != layout::FLAGS_VALUE
        || node.header.flags_extension.is_some()
        || !node.header.stamps.is_empty()
        || !matches!(node.outline_value, 3 | 9 | 15)
        || node.value != layout::VALUE_VALUE
        || node.properties.marker() != layout::MARKER_VALUE
        || node.properties.metadata()
            != Some(&PmDcListMetadata::U32([
                layout::METADATA0_VALUE,
                layout::METADATA1_VALUE,
            ]))
        || node_label.class_id() != CLASS
        || !node_label.header.owner.qualified()
        || node_label.identity.type_id
            != RecordTypeId::from_bytes(crate::feature::FEATURE_LABEL_TYPE)
        || node_label.participants.marker() != 2
        || !node_label.participants.references().is_empty()
        || node
            .properties
            .references()
            .iter()
            .any(|reference| !reference.qualified() || reference.index() == 0)
    {
        return None;
    }
    let [mode, source_index, owner_index, output] = node.properties.references() else {
        return None;
    };
    if index.record_lengths.as_ref().is_some_and(|lengths| {
        lengths.get(&(token, node.identity.record_ordinal))
            != Some(&Some(cadmpeg_core::decode::u64_from_index(layout::LEN)))
    }) {
        return None;
    }
    for (reference, expected) in [mode, source_index, owner_index].into_iter().zip([
        GENERATED_MODE,
        source_body.header.source_index,
        mirror.header.source_index,
    ]) {
        let property = index.property(token, *reference)?;
        if !version19_property(property)
            || !matches!(property.kind,
                PmDcFeaturePropertyKind::RdxVariable {
                    name_value: 0,
                    nominal_value,
                    model_value,
                    ..
                } if nominal_value == expected && model_value == expected)
        {
            return None;
        }
    }
    let output = index.property(token, *output)?;
    if !version19_property(output)
        || output.id() == source_body.id()
        || external_body(output)? == source_reference
    {
        return None;
    }
    Some(output)
}

#[cfg(test)]
mod tests;
