// SPDX-License-Identifier: Apache-2.0
//! Neutral document string attributes own the native custom-property records.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::CadIr;

use crate::native::SldprtNative;
use crate::records::{Feature, FeatureHistory};

pub(super) fn represents(attribute: &SourceAttribute) -> bool {
    attribute.target == AttributeTarget::Document
        && !attribute.name.trim().is_empty()
        && matches!(
            attribute.values.as_slice(),
            [Some(AttributeValue::String(_))]
        )
        && (attribute
            .id
            .as_str()
            .starts_with("sldprt:history:custom-property#")
            || !attribute.id.as_str().starts_with("sldprt:"))
}

pub(super) fn synchronize(ir: &CadIr, native: &mut Option<SldprtNative>) -> Result<(), CodecError> {
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
        .filter(|attribute| represents(attribute))
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
    let mut ordinal = history
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .unwrap_or(0);
    for attribute in attributes {
        // (O-capacity): each native history ordinal occupies a u32 field.
        ordinal = ordinal.checked_add(1).ok_or_else(|| {
            CodecError::NotImplemented("SLDPRT custom-property ordinal exceeds u32".into())
        })?;
        let [Some(AttributeValue::String(value))] = attribute.values.as_slice() else {
            return Err(CodecError::malformed(
                "custom property requires one string value",
            ));
        };
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
            text: Some(value.clone()),
            content: vec![crate::records::FeatureContent::Text(value.clone())],
        });
    }
    Ok(())
}
