// SPDX-License-Identifier: Apache-2.0
//! Current expression link ordinals are emitted from canonical held link slots.

use crate::native::ValueRecord;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn normalize(
    kind: &str,
    values: &[ValueRecord],
) -> Result<(Vec<ValueRecord>, bool), CodecError> {
    let mut result = values.to_vec();
    if !crate::persistence::document_properties::value_parts::links::expression_container(kind) {
        return Ok((result, false));
    }
    let containers = values
        .iter()
        .filter(|value| value.tag == "XLinks")
        .collect::<Vec<_>>();
    let mut omitted = BTreeSet::new();
    let mut lost = false;
    for container in containers {
        let links = values
            .iter()
            .filter(|value| value.parent == Some(container.order) && value.tag == "XLink")
            .collect::<Vec<_>>();
        let declared = container
            .attributes
            .get("count")
            .map(|text| {
                text.parse::<usize>()
                    .map_err(|_| CodecError::malformed("expression link count is invalid"))
            })
            .transpose()?;
        let authored_dense =
            links.iter().all(|value| value.link_index.is_none()) && declared == Some(links.len());
        let mut ordinals = BTreeMap::new();
        for (dense, link) in links.iter().enumerate() {
            if let Some(original) = link.link_index.or(authored_dense.then_some(dense)) {
                if ordinals.insert(original, dense).is_some() {
                    return Err(CodecError::malformed("expression link slot is restated"));
                }
            }
        }
        let maps = values
            .iter()
            .filter(|value| value.parent == Some(container.order) && value.tag == "DocMap")
            .collect::<Vec<_>>();
        let mut docs = 0_usize;
        for map in maps {
            let original = map
                .attributes
                .get("index")
                .ok_or_else(|| CodecError::malformed("expression document map index is absent"))?
                .parse::<usize>()
                .map_err(|_| CodecError::malformed("expression document map index is invalid"))?;
            if let Some(dense) = ordinals.get(&original) {
                let current = result
                    .iter_mut()
                    .find(|value| value.order == map.order)
                    .ok_or_else(|| CodecError::malformed("document map row is absent"))?;
                current.attributes.insert("index".into(), dense.to_string());
                docs = docs
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("document map count overflows"))?;
            } else {
                omitted.insert(map.order);
                lost = true;
            }
        }
        let current = result
            .iter_mut()
            .find(|value| value.order == container.order)
            .ok_or_else(|| CodecError::malformed("expression link container is absent"))?;
        current
            .attributes
            .insert("count".into(), links.len().to_string());
        if docs == 0 {
            current.attributes.remove("docs");
        } else {
            current.attributes.insert("docs".into(), docs.to_string());
        }
        if let Some(hidden) = container.attributes.get("hidden") {
            let mut retained = Vec::new();
            for token in hidden.split_whitespace() {
                let original = token.parse::<usize>().map_err(|_| {
                    CodecError::malformed("expression hidden link index is invalid")
                })?;
                if let Some(dense) = ordinals.get(&original) {
                    retained.push(dense.to_string());
                } else {
                    lost = true;
                }
            }
            if retained.is_empty() {
                current.attributes.remove("hidden");
            } else {
                current
                    .attributes
                    .insert("hidden".into(), retained.join(" "));
            }
        }
    }
    result.retain(|value| !omitted.contains(&value.order));
    Ok((result, lost))
}
