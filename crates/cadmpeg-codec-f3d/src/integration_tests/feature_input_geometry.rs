// SPDX-License-Identifier: Apache-2.0
//! The full decode of CC0 Fusion saves keeps the sampled geometry of every
//! input-state edge that a feature selects.

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// Every string that a JSON value holds, at any depth.
fn strings(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(text) => {
            out.insert(text.clone());
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        serde_json::Value::Object(fields) => fields.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

/// The ids that the members of a topology field list.
fn ids(topology: &serde_json::Value, field: &str, member: Option<&str>) -> BTreeSet<String> {
    topology[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field}"))
        .iter()
        .map(|item| member.map_or(item, |name| &item[name]))
        .map(|id| id.as_str().expect("member id").to_owned())
        .collect()
}

/// The number of input edges and faces that the features select, after
/// asserting that each selected edge has geometry and that each feature that
/// selects an input face keeps edge geometry.
fn selected_edges_with_geometry(bytes: &[u8]) -> usize {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let model = &result.ir().model;
    let mut checked = 0;
    for topology in &model.feature_input_topologies {
        let topology = json::to_value(topology).unwrap();
        let feature = model
            .features
            .iter()
            .find(|feature| json::to_value(&feature.id).unwrap() == topology["input_of"])
            .expect("the feature of an input topology");
        let mut named = BTreeSet::new();
        strings(&json::to_value(feature).unwrap(), &mut named);
        let selected: BTreeSet<_> = ids(&topology, "edges", None)
            .intersection(&named)
            .cloned()
            .collect();
        let sampled = ids(&topology, "edge_geometry", Some("edge"));
        let missing: Vec<_> = selected.difference(&sampled).collect();
        assert!(
            missing.is_empty(),
            "{}: selected input edges without geometry: {missing:?}",
            topology["input_of"]
        );
        let selects_face = ids(&topology, "faces", None)
            .intersection(&named)
            .next()
            .is_some();
        assert!(
            !selects_face || !sampled.is_empty(),
            "{}: a selected input face has no boundary edge geometry",
            topology["input_of"]
        );
        checked += selected.len() + usize::from(selects_face);
    }
    checked
}

#[test]
fn selected_input_edges_keep_their_geometry_through_the_full_decode() {
    let checked = [
        include_bytes!("../../../../corpus/f3d/combine_join_then_fillet.f3d").as_slice(),
        include_bytes!("../../../../corpus/f3d/timeline_rolled_after_edit.f3d").as_slice(),
        include_bytes!("../../../../corpus/f3d/timeline_combine_suppressed.f3d").as_slice(),
    ]
    .into_iter()
    .map(selected_edges_with_geometry)
    .sum::<usize>();
    assert!(checked > 0, "the saves select input-state edges");
}
