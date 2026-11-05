// SPDX-License-Identifier: Apache-2.0
//! Neutral document string attributes own the native custom-property records.

use std::collections::BTreeMap;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::CadIr;

use crate::native::SldprtNative;
use crate::records::{Feature, FeatureHistory};

pub(super) fn represents(attribute: &SourceAttribute) -> bool {
    represented_value(attribute).is_some()
}

fn represented_value(attribute: &SourceAttribute) -> Option<&str> {
    if attribute.target == AttributeTarget::Document
        && !attribute.name.trim().is_empty()
        && (attribute
            .id
            .as_str()
            .starts_with("sldprt:history:custom-property#")
            || !attribute.id.as_str().starts_with("sldprt:"))
    {
        if let [Some(AttributeValue::String(value))] = attribute.values.as_slice() {
            return Some(value);
        }
    }
    None
}

pub(super) const HISTORY_ORDINAL_CAPACITY: cadmpeg_ir::codec::write::CapacityField =
    cadmpeg_ir::codec::write::CapacityField {
        field: "custom_property.ordinal",
        limit: u32::MAX as u64,
        source: "docs/formats/sldprt.md §2: Keywords history order",
        helper: "synchronize",
    };

pub(super) fn synchronize(
    ir: &CadIr,
    native: &mut Option<SldprtNative>,
) -> Result<(), crate::writer::write_error::WriteError> {
    if let Some(native) = native.as_mut().filter(|_| ir.scope.model) {
        for history in &mut native.feature_histories {
            history
                .features
                .retain(|feature| !feature.xml_tag.eq_ignore_ascii_case("CustomProperty"));
            let identities = history
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            history.content.retain(|item| match item {
                crate::records::HistoryContent::Feature(id) => identities.contains(id.as_str()),
                _ => true,
            });
        }
    }
    let attributes = ir
        .model
        .attributes
        .iter()
        .filter_map(|attribute| represented_value(attribute).map(|value| (attribute, value)))
        .collect::<Vec<_>>();
    if attributes.is_empty() {
        return Ok(());
    }
    let native = native.get_or_insert_with(SldprtNative::default);
    if native.feature_histories.is_empty() {
        native.feature_histories.push(FeatureHistory {
            id: "sldprt:generated:feature-history#0".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: Vec::new(),
        });
    }
    let history = &mut native.feature_histories[0];
    let mut order = (0..history.features.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| (history.features[*index].ordinal, *index));
    for (rank, index) in order.into_iter().enumerate() {
        history.features[index].ordinal =
            crate::writer::field_u32(&HISTORY_ORDINAL_CAPACITY, rank)?;
    }
    for (attribute, value) in attributes {
        let ordinal = crate::writer::field_u32(&HISTORY_ORDINAL_CAPACITY, history.features.len())?;
        history.features.push(Feature {
            id: format!(
                "sldprt:generated:custom-property#{}",
                attribute
                    .id
                    .as_str()
                    .replace('%', "%25")
                    .replace('#', "%23")
            ),
            parent: history.id.clone(),
            xml_tag: "CustomProperty".into(),
            tree_parent: None,
            source_id: None,
            ordinal,
            name: attribute.name.clone(),
            kind: "CustomProperty".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: Some(value.to_owned()),
            content: vec![crate::records::FeatureContent::Text(value.to_owned())],
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_populated_custom_properties_redecode() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use std::io::Cursor;

        let mut ir = CadIr::empty();
        for (name, value) in [("Comment", ""), ("Label", "Example")] {
            ir.model.attributes.push(SourceAttribute {
                id: cadmpeg_ir::ids::AttributeId::mint(format!("test:model:attribute#{name}"))
                    .unwrap(),
                native_ref: None,
                target: AttributeTarget::Document,
                name: name.into(),
                values: vec![Some(AttributeValue::String(value.into()))],
            });
        }
        let plan = crate::SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = crate::SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        for attribute in &ir.model.attributes {
            assert!(
                decoded
                    .ir()
                    .model
                    .attributes
                    .iter()
                    .any(|value| value.name == attribute.name && value.values == attribute.values),
                "{}",
                attribute.name
            );
        }
    }

    #[test]
    fn custom_property_ordinal_capacity_is_declared() {
        assert!(crate::writer::CAPACITY_FIELDS.contains(&HISTORY_ORDINAL_CAPACITY));
    }

    #[test]
    fn custom_properties_follow_sparse_feature_ordinals() {
        let mut ir = CadIr::empty();
        ir.model.attributes.push(SourceAttribute {
            id: cadmpeg_ir::ids::AttributeId::mint("test:document:attribute#author").unwrap(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "author".into(),
            values: vec![Some(AttributeValue::String("Ada".into()))],
        });
        let mut native = Some(SldprtNative {
            feature_histories: vec![FeatureHistory {
                id: "test:history#0".into(),
                part_name: None,
                properties: BTreeMap::new(),
                content: Vec::new(),
                configurations: Vec::new(),
                features: vec![Feature {
                    id: "test:feature#0".into(),
                    parent: "test:history#0".into(),
                    xml_tag: "Feature".into(),
                    tree_parent: None,
                    source_id: None,
                    ordinal: u32::MAX,
                    name: "Feature".into(),
                    kind: "Feature".into(),
                    input_class: None,
                    suppressed: false,
                    parameters: BTreeMap::new(),
                    dimension_properties: BTreeMap::new(),
                    properties: BTreeMap::new(),
                    text: None,
                    content: Vec::new(),
                }],
            }],
            ..SldprtNative::default()
        });
        synchronize(&ir, &mut native).unwrap();
        let features = &native.unwrap().feature_histories[0].features;
        assert_eq!(features[0].ordinal, 0);
        assert_eq!(features[1].ordinal, 1);
        assert_eq!(features[1].text.as_deref(), Some("Ada"));
    }
}
